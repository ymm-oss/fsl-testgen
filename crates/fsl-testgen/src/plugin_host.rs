// SPDX-License-Identifier: Apache-2.0

//! Finding and running external target executables (plugin protocol 1).
//!
//! The host sends one request on the plugin's stdin and reads one response
//! from its stdout. It never lets the plugin write: the response is checked
//! by `fsl_testgen_core::plugin` and written by `fsl_testgen_core::output`.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub const CONFIG_FILE: &str = "fsl-testgen.toml";
pub const EXECUTABLE_PREFIX: &str = "fsl-testgen-target-";
/// How long a plugin may take unless `--plugin-timeout` or the
/// configuration says otherwise.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
/// The largest response read from a plugin.
const RESPONSE_LIMIT: u64 = 64 * 1024 * 1024;
/// How much of a failing plugin's stderr an error message quotes.
const STDERR_QUOTE: usize = 2000;

/// A target name is what `--target` takes and what an executable or a
/// configuration entry is named after.
#[must_use]
pub fn valid_target_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric())
        && characters.all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

/// One `[targets.NAME]` entry of `fsl-testgen.toml`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfiguredTarget {
    pub command: Vec<String>,
    pub timeout: Option<Duration>,
}

/// A parsed `fsl-testgen.toml` and the directory it was found in.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Config {
    pub path: PathBuf,
    pub targets: BTreeMap<String, ConfiguredTarget>,
}

/// Parse the text of a configuration file.
///
/// # Errors
///
/// Rejects text that is not TOML, any member other than `targets`, a target
/// name that is not a valid name or is a built-in target, an entry without a
/// non-empty `command` array of strings, and any other member of an entry
/// than `command` and `timeout_seconds`.
pub fn parse_config(path: &Path, text: &str, builtin: &[String]) -> Result<Config, String> {
    let file = path.display();
    let table = text
        .parse::<toml::Table>()
        .map_err(|error| format!("{file} is not TOML: {}", error.to_string().trim_end()))?;
    if let Some(key) = table.keys().find(|key| key.as_str() != "targets") {
        return Err(format!(
            "{file} has unknown member '{key}'; expected [targets.NAME]"
        ));
    }
    let mut targets = BTreeMap::new();
    let Some(entries) = table.get("targets") else {
        return Ok(Config {
            path: path.to_path_buf(),
            targets,
        });
    };
    let entries = entries
        .as_table()
        .ok_or_else(|| format!("{file}: targets must be a table of [targets.NAME] entries"))?;
    for (name, entry) in entries {
        let context = format!("{file}: [targets.{name}]");
        if !valid_target_name(name) {
            return Err(format!(
                "{context} is not a target name (letters, digits, '-', '_' and '.', starting with a letter or digit)"
            ));
        }
        if builtin.contains(name) {
            return Err(format!(
                "{context} names a built-in target; built-in targets cannot be replaced"
            ));
        }
        let entry = entry
            .as_table()
            .ok_or_else(|| format!("{context} must be a table"))?;
        if let Some(key) = entry
            .keys()
            .find(|key| !matches!(key.as_str(), "command" | "timeout_seconds"))
        {
            return Err(format!(
                "{context} has unknown member '{key}'; expected command and timeout_seconds"
            ));
        }
        let command = entry
            .get("command")
            .and_then(toml::Value::as_array)
            .filter(|command| !command.is_empty())
            .and_then(|command| {
                command
                    .iter()
                    .map(|argument| argument.as_str().map(str::to_owned))
                    .collect::<Option<Vec<_>>>()
            })
            .ok_or_else(|| format!("{context}.command must be a non-empty array of strings"))?;
        let timeout = match entry.get("timeout_seconds") {
            None => None,
            Some(value) => Some(Duration::from_secs(
                value
                    .as_integer()
                    .and_then(|seconds| u64::try_from(seconds).ok())
                    .filter(|seconds| *seconds > 0)
                    .ok_or_else(|| {
                        format!("{context}.timeout_seconds must be a positive integer")
                    })?,
            )),
        };
        targets.insert(name.clone(), ConfiguredTarget { command, timeout });
    }
    Ok(Config {
        path: path.to_path_buf(),
        targets,
    })
}

/// The nearest `fsl-testgen.toml` in `start` or a directory above it.
///
/// # Errors
///
/// Returns the reason a file that exists cannot be read or is not valid.
pub fn find_config(start: &Path, builtin: &[String]) -> Result<Option<Config>, String> {
    for directory in start.ancestors() {
        let path = directory.join(CONFIG_FILE);
        if path.is_file() {
            let text = std::fs::read_to_string(&path)
                .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
            return parse_config(&path, &text, builtin).map(Some);
        }
    }
    Ok(None)
}

/// A plugin ready to run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Plugin {
    pub command: Vec<String>,
    /// The directory the plugin runs in; the caller's when `None`.
    pub directory: Option<PathBuf>,
    pub timeout: Option<Duration>,
    /// Where the plugin was found, for messages.
    pub origin: String,
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn on_path(executable: &str, search_path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let candidates = if cfg!(windows) {
        vec![format!("{executable}.exe"), executable.to_owned()]
    } else {
        vec![executable.to_owned()]
    };
    std::env::split_paths(search_path?)
        .flat_map(|directory| {
            candidates
                .iter()
                .map(move |candidate| directory.join(candidate))
                .collect::<Vec<_>>()
        })
        .find(|candidate| is_executable(candidate))
}

/// The plugin for target `name`: the entry of `config` when it has one, else
/// `fsl-testgen-target-NAME` on `search_path`, else none.
///
/// A configured command runs in the directory of the configuration file, so
/// that its relative arguments mean the same from every working directory.
#[must_use]
pub fn resolve(
    name: &str,
    config: Option<&Config>,
    search_path: Option<&std::ffi::OsStr>,
) -> Option<Plugin> {
    if !valid_target_name(name) {
        return None;
    }
    if let Some(config) = config
        && let Some(target) = config.targets.get(name)
    {
        return Some(Plugin {
            command: target.command.clone(),
            directory: config.path.parent().map(Path::to_path_buf),
            timeout: target.timeout,
            origin: format!("[targets.{name}] in {}", config.path.display()),
        });
    }
    let executable = on_path(&format!("{EXECUTABLE_PREFIX}{name}"), search_path)?;
    Some(Plugin {
        origin: executable.display().to_string(),
        command: vec![executable.to_string_lossy().into_owned()],
        directory: None,
        timeout: None,
    })
}

fn quoted_stderr(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let text = text.trim();
    if text.is_empty() {
        return String::new();
    }
    let start = text
        .char_indices()
        .rev()
        .nth(STDERR_QUOTE - 1)
        .map_or(0, |(index, _)| index);
    format!("; stderr: {}", &text[start..])
}

fn read_limited(mut pipe: impl Read + Send + 'static) -> mpsc::Receiver<std::io::Result<Vec<u8>>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = pipe
            .by_ref()
            .take(RESPONSE_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = sender.send(result);
    });
    receiver
}

/// Run `plugin` once: `request` on its stdin, its stdout as the result.
///
/// # Errors
///
/// Returns the reason when the plugin cannot be started, does not finish
/// within `timeout` (it is killed), exits with a status other than 0, or
/// prints more than the response limit or text that is not UTF-8.
pub fn run(plugin: &Plugin, request: &str, timeout: Duration) -> Result<String, String> {
    let (program, arguments) = plugin
        .command
        .split_first()
        .ok_or_else(|| "the plugin command is empty".to_owned())?;
    let mut command = Command::new(program);
    command
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(directory) = &plugin.directory {
        command.current_dir(directory);
    }
    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot start plugin {}: {error}", plugin.origin))?;
    let deadline = Instant::now() + timeout;

    let mut stdin = child.stdin.take().expect("piped stdin");
    let request = request.to_owned();
    std::thread::spawn(move || {
        // A plugin may exit without reading its input; that is its answer.
        let _ = stdin.write_all(request.as_bytes());
    });
    let stdout = read_limited(child.stdout.take().expect("piped stdout"));
    let stderr = read_limited(child.stderr.take().expect("piped stderr"));

    let timed_out = |child: &mut std::process::Child| {
        let _ = child.kill();
        let _ = child.wait();
        format!(
            "plugin {} did not finish within {} s and was stopped",
            plugin.origin,
            timeout.as_secs_f64()
        )
    };
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => return Err(timed_out(&mut child)),
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(error) => {
                let _ = child.kill();
                return Err(format!("cannot wait for plugin {}: {error}", plugin.origin));
            }
        }
    };
    // A process the plugin left behind can keep the pipes open; the deadline
    // covers that too.
    let remaining = || deadline.saturating_duration_since(Instant::now());
    let Ok(stdout) = stdout.recv_timeout(remaining()) else {
        return Err(timed_out(&mut child));
    };
    let stderr = stderr
        .recv_timeout(remaining())
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();
    if !status.success() {
        return Err(format!(
            "plugin {} exited with {}{}",
            plugin.origin,
            status
                .code()
                .map_or_else(|| "a signal".to_owned(), |code| format!("status {code}")),
            quoted_stderr(&stderr)
        ));
    }
    let stdout =
        stdout.map_err(|error| format!("cannot read from plugin {}: {error}", plugin.origin))?;
    if stdout.len() as u64 > RESPONSE_LIMIT {
        return Err(format!(
            "plugin {} printed more than {RESPONSE_LIMIT} bytes",
            plugin.origin
        ));
    }
    String::from_utf8(stdout)
        .map_err(|_| format!("plugin {} printed text that is not UTF-8", plugin.origin))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builtin() -> Vec<String> {
        vec!["pytest".to_owned(), "kotlin".to_owned()]
    }

    #[test]
    fn target_names_are_plain() {
        for name in ["go", "go-test", "x_1", "junit5", "a.b"] {
            assert!(valid_target_name(name), "{name}");
        }
        for name in ["", "-go", ".go", "a/b", "a b", "../x", "a\\b", "é"] {
            assert!(!valid_target_name(name), "{name}");
        }
    }

    #[test]
    fn a_configuration_maps_names_to_commands() {
        let config = parse_config(
            Path::new("/project/fsl-testgen.toml"),
            "[targets.go]\ncommand = [\"python3\", \"tools/go.py\"]\ntimeout_seconds = 5\n\n[targets.junit]\ncommand = [\"fsl-junit\"]\n",
            &builtin(),
        )
        .expect("parse");
        assert_eq!(
            config.targets["go"],
            ConfiguredTarget {
                command: vec!["python3".to_owned(), "tools/go.py".to_owned()],
                timeout: Some(Duration::from_secs(5)),
            }
        );
        assert_eq!(config.targets["junit"].timeout, None);
        let plugin = resolve("go", Some(&config), None).expect("configured");
        assert_eq!(plugin.directory, Some(PathBuf::from("/project")));
        assert_eq!(plugin.origin, "[targets.go] in /project/fsl-testgen.toml");
        assert!(resolve("rust", Some(&config), None).is_none());
        assert!(resolve("../go", Some(&config), None).is_none());
        let empty = parse_config(Path::new("c.toml"), "", &builtin()).expect("parse");
        assert!(empty.targets.is_empty());
    }

    #[test]
    fn a_configuration_is_closed_and_cannot_name_a_built_in_target() {
        let path = Path::new("fsl-testgen.toml");
        for (text, expected) in [
            (
                "[targets.pytest]\ncommand = [\"mine\"]\n",
                "fsl-testgen.toml: [targets.pytest] names a built-in target; built-in targets cannot be replaced",
            ),
            (
                "[target.go]\ncommand = [\"x\"]\n",
                "fsl-testgen.toml has unknown member 'target'; expected [targets.NAME]",
            ),
            (
                "[targets.go]\n",
                "fsl-testgen.toml: [targets.go].command must be a non-empty array of strings",
            ),
            (
                "[targets.go]\ncommand = []\n",
                "fsl-testgen.toml: [targets.go].command must be a non-empty array of strings",
            ),
            (
                "[targets.go]\ncommand = \"go run .\"\n",
                "fsl-testgen.toml: [targets.go].command must be a non-empty array of strings",
            ),
            (
                "[targets.go]\ncommand = [\"x\"]\nshell = true\n",
                "fsl-testgen.toml: [targets.go] has unknown member 'shell'; expected command and timeout_seconds",
            ),
            (
                "[targets.go]\ncommand = [\"x\"]\ntimeout_seconds = 0\n",
                "fsl-testgen.toml: [targets.go].timeout_seconds must be a positive integer",
            ),
            (
                "[targets.\"a b\"]\ncommand = [\"x\"]\n",
                "fsl-testgen.toml: [targets.a b] is not a target name (letters, digits, '-', '_' and '.', starting with a letter or digit)",
            ),
        ] {
            assert_eq!(
                parse_config(path, text, &builtin()).expect_err(text),
                expected
            );
        }
        assert!(
            parse_config(path, "[targets", &builtin())
                .expect_err("not TOML")
                .starts_with("fsl-testgen.toml is not TOML: ")
        );
    }
}
