// SPDX-License-Identifier: Apache-2.0

//! The command line.
//!
//! Exit codes and the error shape are part of the interface:
//!
//! | code | kind     | meaning                                               |
//! |------|----------|-------------------------------------------------------|
//! | 0    |          | generated                                             |
//! | 2    | `usage`  | bad arguments, or a target that does not exist        |
//! | 3    | `input`  | the document is not JSON or failed a cross-check      |
//! | 4    | `target` | the target refused the options or the document        |
//! | 5    | `io`     | the input could not be read or the output written     |
//!
//! A failure prints nothing on stdout and one JSON object on one line of
//! stderr: `{"result":"error","kind":KIND,"message":MESSAGE}`. Nothing is
//! written to the output file unless the exit code is 0.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use fsl_testgen_core::{GeneratedFile, Options, TestgenInput, UnwiredAdapter, parse_document};
use serde_json::json;

use crate::targets::{builtin_target, builtin_targets};

pub const USAGE: &str = "usage: fsl-testgen generate --target TARGET [--input FILE] [-o FILE] [--allow-unwired] [--spec-path PATH]\n       fsl-testgen targets\n       fsl-testgen --version";

/// Why a run ended without generating.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureKind {
    Usage,
    Input,
    Target,
    Io,
}

impl FailureKind {
    #[must_use]
    pub fn exit_code(self) -> u8 {
        match self {
            Self::Usage => 2,
            Self::Input => 3,
            Self::Target => 4,
            Self::Io => 5,
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Usage => "usage",
            Self::Input => "input",
            Self::Target => "target",
            Self::Io => "io",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Failure {
    pub kind: FailureKind,
    pub message: String,
}

fn failure(kind: FailureKind, message: impl Into<String>) -> Failure {
    Failure {
        kind,
        message: message.into(),
    }
}

fn usage(message: impl Into<String>) -> Failure {
    failure(FailureKind::Usage, message)
}

#[derive(Debug, Default, Eq, PartialEq)]
struct GenerateArguments {
    target: Option<String>,
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    spec_path: Option<PathBuf>,
    allow_unwired: bool,
}

fn parse_generate(arguments: &[String]) -> Result<GenerateArguments, Failure> {
    fn set<T>(slot: &mut Option<T>, flag: &str, value: Option<T>) -> Result<(), Failure> {
        let value = value.ok_or_else(|| usage(format!("{flag} needs a value")))?;
        if slot.replace(value).is_some() {
            return Err(usage(format!("{flag} is given more than once")));
        }
        Ok(())
    }
    let mut parsed = GenerateArguments::default();
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--target" => set(&mut parsed.target, "--target", rest.next().cloned())?,
            "--input" => set(&mut parsed.input, "--input", rest.next().map(PathBuf::from))?,
            "-o" | "--output" => set(&mut parsed.output, "-o", rest.next().map(PathBuf::from))?,
            "--spec-path" => set(
                &mut parsed.spec_path,
                "--spec-path",
                rest.next().map(PathBuf::from),
            )?,
            "--allow-unwired" => parsed.allow_unwired = true,
            other => return Err(usage(format!("unknown argument '{other}'\n{USAGE}"))),
        }
    }
    if parsed.target.is_none() {
        return Err(usage(format!("--target is required\n{USAGE}")));
    }
    Ok(parsed)
}

/// Write `content` to `path` through a sibling temporary file, so that the
/// file is either the whole new content or untouched.
fn write_atomically(path: &Path, content: &str) -> Result<(), String> {
    let name = path
        .file_name()
        .ok_or_else(|| format!("{} is not a file path", path.display()))?;
    let mut temporary_name = std::ffi::OsString::from(".");
    temporary_name.push(name);
    temporary_name.push(format!(".fsl-testgen-{}.tmp", std::process::id()));
    let temporary = path.with_file_name(temporary_name);
    let written = std::fs::write(&temporary, content)
        .and_then(|()| std::fs::rename(&temporary, path))
        .map_err(|error| format!("cannot write {}: {error}", path.display()));
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    written
}

fn generate(
    arguments: &[String],
    stdin: &mut dyn Read,
    stdout: &mut dyn Write,
) -> Result<(), Failure> {
    let arguments = parse_generate(arguments)?;
    let target_name = arguments.target.as_deref().unwrap_or_default();
    let target = builtin_target(target_name).ok_or_else(|| {
        usage(format!(
            "unknown target '{target_name}'; expected {}",
            target_names().join(", ")
        ))
    })?;

    let mut text = String::new();
    match &arguments.input {
        Some(path) => {
            text = std::fs::read_to_string(path).map_err(|error| {
                failure(
                    FailureKind::Io,
                    format!("cannot read {}: {error}", path.display()),
                )
            })?;
        }
        None => {
            stdin
                .read_to_string(&mut text)
                .map_err(|error| failure(FailureKind::Io, format!("cannot read stdin: {error}")))?;
        }
    }
    let document = parse_document(&text).map_err(|error| failure(FailureKind::Input, error))?;
    let input = TestgenInput::from_document(&document)
        .map_err(|error| failure(FailureKind::Input, error))?;

    let options = Options {
        unwired: if arguments.allow_unwired {
            UnwiredAdapter::Skip
        } else {
            UnwiredAdapter::Fail
        },
        spec_path: arguments.spec_path.clone(),
        output: arguments.output.clone(),
    };
    let files = target
        .generate(&input, &options)
        .map_err(|error| failure(FailureKind::Target, error))?;
    let [GeneratedFile { content, .. }] = files.as_slice() else {
        return Err(failure(
            FailureKind::Target,
            format!(
                "target '{target_name}' generated {} files; one is expected",
                files.len()
            ),
        ));
    };
    let io = |error: std::io::Error| failure(FailureKind::Io, format!("cannot write: {error}"));
    match &arguments.output {
        None => stdout.write_all(content.as_bytes()).map_err(io)?,
        Some(path) => {
            write_atomically(path, content).map_err(|error| failure(FailureKind::Io, error))?;
            writeln!(
                stdout,
                "{}",
                json!({
                    "result": "generated",
                    "target": target_name,
                    "spec": input.spec_name(),
                    "output": path,
                })
            )
            .map_err(io)?;
        }
    }
    Ok(())
}

fn target_names() -> Vec<String> {
    builtin_targets()
        .iter()
        .map(|target| target.name().to_owned())
        .collect()
}

fn dispatch(
    arguments: &[String],
    stdin: &mut dyn Read,
    stdout: &mut dyn Write,
) -> Result<(), Failure> {
    let io = |error: std::io::Error| failure(FailureKind::Io, format!("cannot write: {error}"));
    match arguments.split_first() {
        Some((command, rest)) if command == "generate" => generate(rest, stdin, stdout),
        Some((command, rest)) if command == "targets" => {
            if let Some(extra) = rest.first() {
                return Err(usage(format!("unknown argument '{extra}'\n{USAGE}")));
            }
            for name in target_names() {
                writeln!(stdout, "{name}").map_err(io)?;
            }
            Ok(())
        }
        Some((flag, _)) if flag == "--version" || flag == "-V" => {
            writeln!(stdout, "fsl-testgen {}", env!("CARGO_PKG_VERSION")).map_err(io)
        }
        Some((flag, _)) if flag == "--help" || flag == "-h" => {
            writeln!(stdout, "{USAGE}").map_err(io)
        }
        Some((command, _)) => Err(usage(format!("unknown command '{command}'\n{USAGE}"))),
        None => Err(usage(USAGE)),
    }
}

/// Run the command line; the return value is the process exit code.
pub fn run(
    arguments: &[String],
    stdin: &mut dyn Read,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    match dispatch(arguments, stdin, stdout) {
        Ok(()) => 0,
        Err(failure) => {
            let _ = writeln!(
                stderr,
                "{}",
                json!({
                    "result": "error",
                    "kind": failure.kind.name(),
                    "message": failure.message,
                })
            );
            failure.kind.exit_code()
        }
    }
}
