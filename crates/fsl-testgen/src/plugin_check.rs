// SPDX-License-Identifier: Apache-2.0

//! `fsl-testgen plugin-check`: what can be checked about an external target
//! without knowing its language.
//!
//! The plugin is run over the golden inputs of this repository, which are
//! compiled into the binary. See `docs/PLUGIN.md` for what the checks mean
//! and, more importantly, for what they cannot show.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::time::Duration;

use fsl_testgen_core::plugin::Response;
use fsl_testgen_core::{Options, TestgenInput, UnwiredAdapter, parse_document};
use serde_json::Value;

use crate::cli::ask;
use crate::plugin_host::Plugin;

macro_rules! golden {
    ($($name:literal),* $(,)?) => {
        [$(($name, include_str!(concat!("../../../tests/golden/", $name, "/input.json")))),*]
    };
}

/// The golden inputs: one per dialect, a forbidden scenario, a compose root
/// (`kernel: null`), a document without scenarios or walk steps, a source
/// name that needs escaping. (The `output` golden is the same document as
/// `escaped-name` under another file name, and is left out.)
pub const INPUTS: [(&str, &str); 9] = golden![
    "kernel",
    "requirements",
    "forbidden",
    "business",
    "db",
    "compose",
    "domain",
    "governance",
    "escaped-name",
];

/// What `plugin-check` cannot check, printed with every report.
pub const NOT_CHECKED: [&str; 4] = [
    "that the generated suite FAILS while its adapter is not wired. A suite that never calls the implementation and still passes is not evidence that the implementation conforms (ymm-oss/fsl issue #1200). Only running the generated suite, unwired, in its own test framework shows this; it is your requirement to meet.",
    "that unwired=\"skip\" really skips instead of passing, or that the plugin refuses it when the framework cannot skip at run time.",
    "that the generated files compile, run, and assert every step, every expected state and the rejection of every forbidden step.",
    "that strings from the document (source name, scenario and action names, string values) are escaped correctly for the language.",
];

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    Ok,
    Fail,
    Note,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Report {
    pub lines: Vec<(Status, String)>,
}

impl Report {
    fn push(&mut self, status: Status, line: String) {
        self.lines.push((status, line));
    }

    #[must_use]
    pub fn failed(&self) -> usize {
        self.lines
            .iter()
            .filter(|(status, _)| *status == Status::Fail)
            .count()
    }

    #[must_use]
    pub fn render(&self) -> String {
        let mut text = String::new();
        for (status, line) in &self.lines {
            let label = match status {
                Status::Ok => "ok  ",
                Status::Fail => "FAIL",
                Status::Note => "note",
            };
            let _ = writeln!(text, "{label}  {line}");
        }
        let checks = self
            .lines
            .iter()
            .filter(|(status, _)| *status != Status::Note)
            .count();
        let _ = writeln!(
            text,
            "\nplugin-check: {checks} checks, {} failed",
            self.failed()
        );
        text.push_str("\nNot checked (plugin-check cannot see these):\n");
        for item in NOT_CHECKED {
            let _ = writeln!(text, "  - {item}");
        }
        text
    }
}

fn mode_name(unwired: UnwiredAdapter) -> &'static str {
    match unwired {
        UnwiredAdapter::Fail => "fail",
        UnwiredAdapter::Skip => "skip",
    }
}

/// Run the plugin twice on `document`; the response when both runs gave the
/// same valid one.
fn answer(
    plugin: &Plugin,
    document: &Value,
    unwired: UnwiredAdapter,
    timeout: Duration,
) -> Result<Response, String> {
    let input = TestgenInput::from_document(document)
        .map_err(|error| format!("internal: the golden input is rejected: {error}"))?;
    let options = Options {
        unwired,
        ..Options::default()
    };
    let first = ask(plugin, &input, &options, timeout)?;
    let second = ask(plugin, &input, &options, timeout)
        .map_err(|error| format!("the second run failed: {error}"))?;
    if first != second {
        return Err("not deterministic: two runs on the same request differ".to_owned());
    }
    Ok(first)
}

fn total_bytes(response: &Response) -> usize {
    response.files.iter().map(|file| file.content.len()).sum()
}

fn check_case(
    report: &mut Report,
    plugin: &Plugin,
    name: &str,
    document: &Value,
    unwired: UnwiredAdapter,
    timeout: Duration,
) {
    let mut label = format!("{name} (unwired={})", mode_name(unwired));
    if document["kernel"].is_null() {
        label.push_str(", a compose input with kernel: null");
    }
    let response = match answer(plugin, document, unwired, timeout) {
        Ok(response) => response,
        Err(error) => return report.push(Status::Fail, format!("{label}: {error}")),
    };
    let errors = response.errors();
    if !errors.is_empty() {
        let refusal = errors.join("; ");
        return match unwired {
            UnwiredAdapter::Fail => report.push(
                Status::Fail,
                format!("{label}: the plugin refuses a golden input: {refusal}"),
            ),
            // Refusing to generate a skipping suite is a legitimate answer.
            UnwiredAdapter::Skip => {
                report.push(Status::Note, format!("{label}: refused: {refusal}"));
            }
        };
    }
    if response.files.is_empty() {
        return report.push(Status::Fail, format!("{label}: no files and no error"));
    }
    if let Some(empty) = response.files.iter().find(|file| file.content.is_empty()) {
        return report.push(Status::Fail, format!("{label}: '{}' is empty", empty.path));
    }
    report.push(
        Status::Ok,
        format!(
            "{label}: valid response, the same on a second run, {} file(s), {} bytes",
            response.files.len(),
            total_bytes(&response)
        ),
    );
    let source = document["source"]["name"].as_str().unwrap_or_default();
    if name == "escaped-name" && unwired == UnwiredAdapter::Fail {
        let verbatim = response
            .files
            .iter()
            .filter(|file| file.content.contains(source))
            .count();
        report.push(
            Status::Note,
            if verbatim == 0 {
                format!("{label}: the source name {source:?} is not printed verbatim")
            } else {
                format!(
                    "{label}: the source name {source:?} is printed verbatim in {verbatim} file(s); make sure that is safe where it is printed"
                )
            },
        );
    }
}

/// A scenario kind of the golden inputs generates something: with only the
/// scenarios of that kind the output differs from the output with none.
fn check_kind(
    report: &mut Report,
    plugin: &Plugin,
    kind: &str,
    case: &str,
    document: &Value,
    timeout: Duration,
) {
    let label = format!("scenario kind {kind} (from {case})");
    let with = |keep: &dyn Fn(&Value) -> bool| {
        let mut document = document.clone();
        if let Some(scenarios) = document["scenarios"].as_array_mut() {
            scenarios.retain(|scenario| keep(scenario));
        }
        answer(plugin, &document, UnwiredAdapter::Fail, timeout)
    };
    let only = with(&|scenario| scenario["kind"].as_str() == Some(kind));
    let none = with(&|_| false);
    match (only, none) {
        (Err(error), _) | (_, Err(error)) => report.push(Status::Fail, format!("{label}: {error}")),
        (Ok(only), Ok(_)) if !only.errors().is_empty() => report.push(
            Status::Fail,
            format!("{label}: refused: {}", only.errors().join("; ")),
        ),
        (Ok(only), Ok(none)) if only.files == none.files => report.push(
            Status::Fail,
            format!("{label}: the output is the same as for a document without scenarios"),
        ),
        (Ok(_), Ok(_)) => report.push(
            Status::Ok,
            format!("{label}: the output differs from a document without scenarios"),
        ),
    }
}

/// Run every check on `plugin`.
#[must_use]
pub fn check(plugin: &Plugin, timeout: Duration) -> Report {
    let mut report = Report::default();
    let mut kinds = BTreeMap::new();
    for (name, text) in INPUTS {
        let document = match parse_document(text) {
            Ok(document) => document,
            Err(error) => {
                report.push(Status::Fail, format!("{name}: internal: {error}"));
                continue;
            }
        };
        for unwired in [UnwiredAdapter::Fail, UnwiredAdapter::Skip] {
            check_case(&mut report, plugin, name, &document, unwired, timeout);
        }
        for scenario in document["scenarios"].as_array().into_iter().flatten() {
            if let Some(kind) = scenario["kind"].as_str() {
                kinds
                    .entry(kind.to_owned())
                    .or_insert_with(|| (name, document.clone()));
            }
        }
    }
    for (kind, (case, document)) in &kinds {
        check_kind(&mut report, plugin, kind, case, document, timeout);
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every scenario kind `fslc scenarios` gives is among the golden inputs,
    /// and every input still passes the reader.
    #[test]
    fn the_embedded_inputs_cover_the_scenario_kinds() {
        let mut kinds = std::collections::BTreeSet::new();
        let mut compose = 0;
        for (name, text) in INPUTS {
            let document = parse_document(text).expect("JSON");
            TestgenInput::from_document(&document)
                .unwrap_or_else(|error| panic!("{name}: {error}"));
            compose += usize::from(document["kernel"].is_null());
            for scenario in document["scenarios"].as_array().expect("scenarios") {
                kinds.insert(scenario["kind"].as_str().expect("kind").to_owned());
            }
        }
        assert_eq!(
            kinds.into_iter().collect::<Vec<_>>(),
            [
                "acceptance",
                "action_coverage",
                "deadlock",
                "forbidden",
                "leadsTo",
                "reachable"
            ]
        );
        assert_eq!(compose, 1);
    }
}
