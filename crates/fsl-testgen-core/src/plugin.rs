// SPDX-License-Identifier: Apache-2.0

//! Plugin protocol 1: what an external target executable is sent and what
//! it may answer. See `docs/PLUGIN.md` and the two schemas in `schemas/`.
//!
//! This module only builds the request and checks the response. Running the
//! executable and writing files is the caller's; a plugin never writes.

use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

use crate::input::TestgenInput;
use crate::output::check_relative_path;
use crate::target::{GeneratedFile, Options, UnwiredAdapter};

/// The protocol version this crate speaks.
pub const PROTOCOL: u64 = 1;

/// How serious a plugin's diagnostic is. An `Error` refuses the generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

impl Severity {
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub severity: Severity,
    pub message: String,
}

/// A response that passed every check of [`parse_response`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Response {
    pub files: Vec<GeneratedFile>,
    pub diagnostics: Vec<Diagnostic>,
}

impl Response {
    /// The messages of the diagnostics with severity `error`.
    #[must_use]
    pub fn errors(&self) -> Vec<&str> {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == Severity::Error)
            .map(|diagnostic| diagnostic.message.as_str())
            .collect()
    }
}

/// The request for one generation: the document as given, and the options.
///
/// Taking a [`TestgenInput`] is the guarantee that a plugin is never sent a
/// document that failed a cross-check: there is no other way to build one.
#[must_use]
pub fn request(input: &TestgenInput, options: &Options) -> Value {
    let path = |path: &Option<std::path::PathBuf>| {
        path.as_ref()
            .map_or(Value::Null, |path| json!(path.to_string_lossy()))
    };
    json!({
        "protocol": PROTOCOL,
        "input": input.document(),
        "options": {
            "unwired": match options.unwired {
                UnwiredAdapter::Fail => "fail",
                UnwiredAdapter::Skip => "skip",
            },
            "spec_path": path(&options.spec_path),
            "output": path(&options.output),
        },
    })
}

fn closed<'a>(
    value: &'a Value,
    context: &str,
    required: &[&str],
) -> Result<&'a Map<String, Value>, String> {
    let object = value
        .as_object()
        .ok_or_else(|| format!("{context} must be an object"))?;
    if let Some(key) = object.keys().find(|key| !required.contains(&key.as_str())) {
        return Err(format!("{context} has unknown member '{key}'"));
    }
    if let Some(key) = required.iter().find(|key| !object.contains_key(**key)) {
        return Err(format!("{context}.{key} is required"));
    }
    Ok(object)
}

fn string<'a>(object: &'a Map<String, Value>, key: &str, context: &str) -> Result<&'a str, String> {
    object[key]
        .as_str()
        .ok_or_else(|| format!("{context}.{key} must be a string"))
}

fn array<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<&'a [Value], String> {
    object[key]
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| format!("{context}.{key} must be an array"))
}

/// Check the text a plugin printed.
///
/// # Errors
///
/// Rejects text that is not one JSON object of protocol 1 with exactly
/// `protocol`, `files` and `diagnostics`; a file without a string `path` and
/// `content`; a path that is absolute, escapes with `..`, or is otherwise not
/// a plain relative path; the same path twice; and a diagnostic whose
/// severity is not `error`, `warning` or `info`.
pub fn parse_response(text: &str) -> Result<Response, String> {
    let value: Value =
        serde_json::from_str(text).map_err(|error| format!("the response is not JSON: {error}"))?;
    let root = closed(&value, "response", &["protocol", "files", "diagnostics"])?;
    if root["protocol"].as_u64() != Some(PROTOCOL) {
        return Err(format!(
            "response.protocol is {}; this fsl-testgen speaks protocol {PROTOCOL}",
            root["protocol"]
        ));
    }
    let mut seen = BTreeSet::new();
    let files = array(root, "files", "response")?
        .iter()
        .enumerate()
        .map(|(index, file)| {
            let context = format!("response.files[{index}]");
            let file = closed(file, &context, &["path", "content"])?;
            let path = string(file, "path", &context)?;
            check_relative_path(path).map_err(|error| format!("{context}.path {error}"))?;
            if !seen.insert(path) {
                return Err(format!("{context}.path '{path}' is given more than once"));
            }
            Ok(GeneratedFile {
                path: path.to_owned(),
                content: string(file, "content", &context)?.to_owned(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let diagnostics = array(root, "diagnostics", "response")?
        .iter()
        .enumerate()
        .map(|(index, diagnostic)| {
            let context = format!("response.diagnostics[{index}]");
            let diagnostic = closed(diagnostic, &context, &["severity", "message"])?;
            let severity = match string(diagnostic, "severity", &context)? {
                "error" => Severity::Error,
                "warning" => Severity::Warning,
                "info" => Severity::Info,
                other => {
                    return Err(format!(
                        "{context}.severity is '{other}'; expected error, warning or info"
                    ));
                }
            };
            Ok(Diagnostic {
                severity,
                message: string(diagnostic, "message", &context)?.to_owned(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(Response { files, diagnostics })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn the_request_carries_the_document_as_given_and_the_options() {
        let document = crate::input::tests::document();
        let input = TestgenInput::from_document(&document).expect("read");
        let request = request(
            &input,
            &Options {
                unwired: UnwiredAdapter::Skip,
                spec_path: Some(PathBuf::from("specs/demo.fsl")),
                output: None,
            },
        );
        assert_eq!(
            request
                .as_object()
                .expect("object")
                .keys()
                .collect::<Vec<_>>(),
            ["protocol", "input", "options"]
        );
        assert_eq!(request["protocol"], 1);
        assert_eq!(
            serde_json::to_string(&request["input"]).expect("serialize"),
            serde_json::to_string(&document).expect("serialize"),
            "member order included"
        );
        assert_eq!(
            request["options"],
            json!({"unwired": "skip", "spec_path": "specs/demo.fsl", "output": null})
        );
        let default = super::request(&input, &Options::default());
        assert_eq!(
            default["options"],
            json!({"unwired": "fail", "spec_path": null, "output": null})
        );
    }

    #[test]
    fn a_well_formed_response_is_read() {
        let response = parse_response(
            r#"{"protocol": 1,
                "files": [{"path": "a/b_test.go", "content": "package a\n"}, {"path": "c", "content": ""}],
                "diagnostics": [{"severity": "warning", "message": "w"}, {"severity": "info", "message": "i"}]}"#,
        )
        .expect("read");
        assert_eq!(response.files.len(), 2);
        assert_eq!(response.files[0].path, "a/b_test.go");
        assert_eq!(response.files[0].content, "package a\n");
        assert!(response.errors().is_empty());
        let refused = parse_response(
            r#"{"protocol": 1, "files": [], "diagnostics": [{"severity": "error", "message": "no"}]}"#,
        )
        .expect("read");
        assert_eq!(refused.errors(), ["no"]);
    }

    #[test]
    fn a_malformed_response_is_rejected_for_its_own_reason() {
        for (text, expected) in [
            (
                "",
                "the response is not JSON: EOF while parsing a value at line 1 column 0",
            ),
            ("[]", "response must be an object"),
            (
                r#"{"protocol": 1, "files": []}"#,
                "response.diagnostics is required",
            ),
            (
                r#"{"protocol": 2, "files": [], "diagnostics": []}"#,
                "response.protocol is 2; this fsl-testgen speaks protocol 1",
            ),
            (
                r#"{"protocol": "1", "files": [], "diagnostics": []}"#,
                "response.protocol is \"1\"; this fsl-testgen speaks protocol 1",
            ),
            (
                r#"{"protocol": 1, "files": [], "diagnostics": [], "extra": 1}"#,
                "response has unknown member 'extra'",
            ),
            (
                r#"{"protocol": 1, "files": {}, "diagnostics": []}"#,
                "response.files must be an array",
            ),
            (
                r#"{"protocol": 1, "files": [{"path": "a"}], "diagnostics": []}"#,
                "response.files[0].content is required",
            ),
            (
                r#"{"protocol": 1, "files": [{"path": "a", "content": 1}], "diagnostics": []}"#,
                "response.files[0].content must be a string",
            ),
            (
                r#"{"protocol": 1, "files": [{"path": "a", "content": "", "mode": 493}], "diagnostics": []}"#,
                "response.files[0] has unknown member 'mode'",
            ),
            (
                r#"{"protocol": 1, "files": [{"path": "/etc/passwd", "content": ""}], "diagnostics": []}"#,
                "response.files[0].path '/etc/passwd' is absolute",
            ),
            (
                r#"{"protocol": 1, "files": [{"path": "a/../../b", "content": ""}], "diagnostics": []}"#,
                "response.files[0].path 'a/../../b' has a '..' component",
            ),
            (
                r#"{"protocol": 1, "files": [{"path": "a", "content": ""}, {"path": "a", "content": ""}], "diagnostics": []}"#,
                "response.files[1].path 'a' is given more than once",
            ),
            (
                r#"{"protocol": 1, "files": [], "diagnostics": [{"severity": "fatal", "message": "m"}]}"#,
                "response.diagnostics[0].severity is 'fatal'; expected error, warning or info",
            ),
            (
                r#"{"protocol": 1, "files": [], "diagnostics": [{"severity": "error"}]}"#,
                "response.diagnostics[0].message is required",
            ),
        ] {
            assert_eq!(parse_response(text).expect_err(text), expected);
        }
    }
}
