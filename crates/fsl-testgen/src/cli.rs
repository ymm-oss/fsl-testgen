// SPDX-License-Identifier: Apache-2.0

//! The command line.
//!
//! Exit codes and the error shape are part of the interface:
//!
//! | code | kind     | meaning                                                  |
//! |------|----------|----------------------------------------------------------|
//! | 0    |          | generated                                                |
//! | 2    | `usage`  | bad arguments, or a target that does not exist           |
//! | 3    | `input`  | the document is not JSON or failed a cross-check         |
//! | 4    | `target` | the target refused the options or the document           |
//! | 5    | `io`     | the input could not be read or the output written        |
//! | 6    | `plugin` | an external target broke the protocol                    |
//! | 7    | `config` | `fsl-testgen.toml` is invalid                            |
//!
//! A failure prints nothing on stdout and one JSON object on one line of
//! stderr: `{"result":"error","kind":KIND,"message":MESSAGE}`. Nothing is
//! written unless the exit code is 0.
//!
//! `plugin-check` is a report for people: it prints one line per check on
//! stdout and exits 0 when every check passed, 1 when one failed.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use fsl_testgen_core::plugin::{Diagnostic, Response, parse_response, request};
use fsl_testgen_core::{
    GeneratedFile, Options, Target, TestgenInput, UnwiredAdapter, output, parse_document,
};
use serde_json::json;

use crate::plugin_check;
use crate::plugin_host::{self, DEFAULT_TIMEOUT, Plugin};
use crate::targets::{builtin_target, builtin_targets};

pub const USAGE: &str = "usage: fsl-testgen generate --target TARGET [--input FILE] [-o FILE | --out-dir DIR] [--allow-unwired] [--spec-path PATH] [--plugin-timeout SECONDS]\n       fsl-testgen targets\n       fsl-testgen plugin-check [--timeout SECONDS] COMMAND [ARGUMENT...]\n       fsl-testgen --version";

/// Why a run ended without generating.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureKind {
    Usage,
    Input,
    Target,
    Io,
    Plugin,
    Config,
}

impl FailureKind {
    #[must_use]
    pub fn exit_code(self) -> u8 {
        match self {
            Self::Usage => 2,
            Self::Input => 3,
            Self::Target => 4,
            Self::Io => 5,
            Self::Plugin => 6,
            Self::Config => 7,
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Usage => "usage",
            Self::Input => "input",
            Self::Target => "target",
            Self::Io => "io",
            Self::Plugin => "plugin",
            Self::Config => "config",
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

/// What the process runs in.
struct Environment {
    directory: PathBuf,
    search_path: Option<OsString>,
}

impl Environment {
    fn current() -> Result<Self, Failure> {
        Ok(Self {
            directory: std::env::current_dir().map_err(|error| {
                failure(
                    FailureKind::Io,
                    format!("cannot tell the working directory: {error}"),
                )
            })?,
            search_path: std::env::var_os("PATH"),
        })
    }
}

#[derive(Debug, Default, Eq, PartialEq)]
struct GenerateArguments {
    target: Option<String>,
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    out_dir: Option<PathBuf>,
    spec_path: Option<PathBuf>,
    plugin_timeout: Option<Duration>,
    allow_unwired: bool,
}

fn set<T>(slot: &mut Option<T>, flag: &str, value: Option<T>) -> Result<(), Failure> {
    let value = value.ok_or_else(|| usage(format!("{flag} needs a value")))?;
    if slot.replace(value).is_some() {
        return Err(usage(format!("{flag} is given more than once")));
    }
    Ok(())
}

fn seconds(flag: &str, value: Option<&String>) -> Result<Option<Duration>, Failure> {
    let Some(value) = value else {
        return Ok(None);
    };
    value
        .parse::<u64>()
        .ok()
        .filter(|seconds| *seconds > 0)
        .map(|seconds| Some(Duration::from_secs(seconds)))
        .ok_or_else(|| usage(format!("{flag} must be a positive number of seconds")))
}

fn parse_generate(arguments: &[String]) -> Result<GenerateArguments, Failure> {
    let mut parsed = GenerateArguments::default();
    let mut rest = arguments.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--target" => set(&mut parsed.target, "--target", rest.next().cloned())?,
            "--input" => set(&mut parsed.input, "--input", rest.next().map(PathBuf::from))?,
            "-o" | "--output" => set(&mut parsed.output, "-o", rest.next().map(PathBuf::from))?,
            "--out-dir" => set(
                &mut parsed.out_dir,
                "--out-dir",
                rest.next().map(PathBuf::from),
            )?,
            "--spec-path" => set(
                &mut parsed.spec_path,
                "--spec-path",
                rest.next().map(PathBuf::from),
            )?,
            "--plugin-timeout" => set(
                &mut parsed.plugin_timeout,
                "--plugin-timeout",
                seconds("--plugin-timeout", rest.next())?,
            )?,
            "--allow-unwired" => parsed.allow_unwired = true,
            other => return Err(usage(format!("unknown argument '{other}'\n{USAGE}"))),
        }
    }
    if parsed.target.is_none() {
        return Err(usage(format!("--target is required\n{USAGE}")));
    }
    if parsed.output.is_some() && parsed.out_dir.is_some() {
        return Err(usage("-o and --out-dir cannot be given together"));
    }
    Ok(parsed)
}

fn builtin_names() -> Vec<String> {
    builtin_targets()
        .iter()
        .map(|target| target.name().to_owned())
        .collect()
}

/// A target found for `--target`: built in, or an external executable.
enum Selected {
    Builtin(Box<dyn Target>),
    External(Plugin),
}

/// A built-in name always selects the built-in target; nothing an
/// executable or a configuration file is called can stand in for one.
fn select(name: &str, environment: &Environment) -> Result<Selected, Failure> {
    // The configuration is read for every generation, so that an entry that
    // tries to replace a built-in target is refused rather than ignored.
    let config = plugin_host::find_config(&environment.directory, &builtin_names())
        .map_err(|error| failure(FailureKind::Config, error))?;
    if let Some(target) = builtin_target(name) {
        return Ok(Selected::Builtin(target));
    }
    plugin_host::resolve(name, config.as_ref(), environment.search_path.as_deref())
        .map(Selected::External)
        .ok_or_else(|| {
            usage(format!(
                "unknown target '{name}'; expected {}, a [targets.{name}] entry in {}, or {}{name} on PATH",
                builtin_names().join(", "),
                plugin_host::CONFIG_FILE,
                plugin_host::EXECUTABLE_PREFIX,
            ))
        })
}

/// Ask an external target for one generation and check its answer.
pub(crate) fn ask(
    plugin: &Plugin,
    input: &TestgenInput,
    options: &Options,
    timeout: Duration,
) -> Result<Response, String> {
    let mut text = request(input, options).to_string();
    text.push('\n');
    let answer = plugin_host::run(plugin, &text, timeout)?;
    parse_response(&answer).map_err(|error| format!("plugin {}: {error}", plugin.origin))
}

fn read_input(path: Option<&Path>, stdin: &mut dyn Read) -> Result<String, Failure> {
    let mut text = String::new();
    match path {
        Some(path) => std::fs::read_to_string(path).map_err(|error| {
            failure(
                FailureKind::Io,
                format!("cannot read {}: {error}", path.display()),
            )
        }),
        None => stdin
            .read_to_string(&mut text)
            .map(|_| text)
            .map_err(|error| failure(FailureKind::Io, format!("cannot read stdin: {error}"))),
    }
}

fn generate(
    arguments: &[String],
    stdin: &mut dyn Read,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<(), Failure> {
    let arguments = parse_generate(arguments)?;
    let target_name = arguments.target.as_deref().unwrap_or_default();
    let selected = select(target_name, &Environment::current()?)?;

    // The document is checked before any target, built in or external, runs.
    let text = read_input(arguments.input.as_deref(), stdin)?;
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
        output: arguments.output.clone().or(arguments.out_dir.clone()),
    };
    let mut notes: Vec<Diagnostic> = Vec::new();
    let (files, broken) = match &selected {
        Selected::Builtin(target) => (
            target
                .generate(&input, &options)
                .map_err(|error| failure(FailureKind::Target, error))?,
            FailureKind::Target,
        ),
        Selected::External(plugin) => {
            let timeout = arguments
                .plugin_timeout
                .or(plugin.timeout)
                .unwrap_or(DEFAULT_TIMEOUT);
            let response = ask(plugin, &input, &options, timeout)
                .map_err(|error| failure(FailureKind::Plugin, error))?;
            let errors = response.errors();
            if !errors.is_empty() {
                return Err(failure(FailureKind::Target, errors.join("; ")));
            }
            notes = response.diagnostics;
            (response.files, FailureKind::Plugin)
        }
    };
    output::check_files(&files).map_err(|error| failure(broken, error))?;
    deliver(&arguments, target_name, &input, &files, stdout)?;
    // Warnings and notes of a plugin, once the generation has succeeded.
    for note in &notes {
        let _ = writeln!(
            stderr,
            "{}",
            json!({
                "result": "diagnostic",
                "severity": note.severity.name(),
                "message": note.message,
            })
        );
    }
    Ok(())
}

/// Print the one file, write it to `-o`, or write every file under
/// `--out-dir`. Nothing is written unless everything can be.
fn deliver(
    arguments: &GenerateArguments,
    target_name: &str,
    input: &TestgenInput,
    files: &[GeneratedFile],
    stdout: &mut dyn Write,
) -> Result<(), Failure> {
    let io = |error: std::io::Error| failure(FailureKind::Io, format!("cannot write: {error}"));
    if let Some(directory) = &arguments.out_dir {
        let written = output::write_tree(directory, files)
            .map_err(|error| failure(FailureKind::Io, error))?;
        return writeln!(
            stdout,
            "{}",
            json!({
                "result": "generated",
                "target": target_name,
                "spec": input.spec_name(),
                "out_dir": directory,
                "files": written,
            })
        )
        .map_err(io);
    }
    let [GeneratedFile { content, .. }] = files else {
        return Err(usage(format!(
            "target '{target_name}' generated {} files; use --out-dir DIR to write them",
            files.len()
        )));
    };
    match &arguments.output {
        None => stdout.write_all(content.as_bytes()).map_err(io),
        Some(path) => {
            output::write_file(path, content).map_err(|error| failure(FailureKind::Io, error))?;
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
            .map_err(io)
        }
    }
}

/// `plugin-check [--timeout SECONDS] COMMAND [ARGUMENT...]`; the exit code.
fn check_plugin(arguments: &[String], stdout: &mut dyn Write) -> Result<u8, Failure> {
    let (timeout, command) = match arguments.split_first() {
        Some((flag, rest)) if flag == "--timeout" => (
            seconds("--timeout", rest.first())?.ok_or_else(|| usage("--timeout needs a value"))?,
            &rest[1..],
        ),
        _ => (DEFAULT_TIMEOUT, arguments),
    };
    if command.is_empty() || command[0].starts_with('-') {
        return Err(usage(format!("plugin-check needs a command\n{USAGE}")));
    }
    let plugin = Plugin {
        origin: command.join(" "),
        command: command.to_vec(),
        directory: None,
        timeout: None,
    };
    let report = plugin_check::check(&plugin, timeout);
    write!(stdout, "{}", report.render())
        .map_err(|error| failure(FailureKind::Io, format!("cannot write: {error}")))?;
    Ok(u8::from(report.failed() > 0))
}

fn dispatch(
    arguments: &[String],
    stdin: &mut dyn Read,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> Result<u8, Failure> {
    let io = |error: std::io::Error| failure(FailureKind::Io, format!("cannot write: {error}"));
    match arguments.split_first() {
        Some((command, rest)) if command == "generate" => {
            generate(rest, stdin, stdout, stderr).map(|()| 0)
        }
        Some((command, rest)) if command == "plugin-check" => check_plugin(rest, stdout),
        Some((command, rest)) if command == "targets" => {
            if let Some(extra) = rest.first() {
                return Err(usage(format!("unknown argument '{extra}'\n{USAGE}")));
            }
            for name in builtin_names() {
                writeln!(stdout, "{name}").map_err(io)?;
            }
            Ok(0)
        }
        Some((flag, _)) if flag == "--version" || flag == "-V" => {
            writeln!(stdout, "fsl-testgen {}", env!("CARGO_PKG_VERSION")).map_err(io)?;
            Ok(0)
        }
        Some((flag, _)) if flag == "--help" || flag == "-h" => {
            writeln!(stdout, "{USAGE}").map_err(io)?;
            Ok(0)
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
    match dispatch(arguments, stdin, stdout, stderr) {
        Ok(code) => code,
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
