// SPDX-License-Identifier: Apache-2.0

//! Shared by the integration tests: the checked-in goldens and the binary.

#![allow(dead_code)]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

pub const TARGETS: [&str; 6] = ["pytest", "vitest", "swift", "kotlin", "dart", "phpunit"];

/// Every case `scripts/regen_goldens.py` writes. A case that disappears from
/// the directory fails the tests instead of shrinking them.
pub const CASES: [&str; 10] = [
    "business",
    "compose",
    "db",
    "domain",
    "escaped-name",
    "forbidden",
    "governance",
    "kernel",
    "output",
    "requirements",
];

pub fn golden_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .join("tests/golden")
}

pub fn repository_root() -> PathBuf {
    golden_root()
        .parent()
        .and_then(Path::parent)
        .expect("repository root")
        .to_owned()
}

pub struct Case {
    pub name: &'static str,
    pub directory: PathBuf,
    pub input: String,
    pub spec_path: String,
    /// The `-o` path of a case generated with one, relative to a scratch root.
    pub output: Option<String>,
    /// The spec path relative to the same root.
    pub spec: String,
}

impl Case {
    pub fn document(&self) -> Value {
        serde_json::from_str(&self.input).expect("golden input is JSON")
    }

    /// The expected bytes of `name`, or the expected refusal message.
    pub fn expected(&self, name: &str) -> Result<Vec<u8>, String> {
        let refusal = self.directory.join(format!("{name}.error"));
        if refusal.exists() {
            let message = std::fs::read_to_string(refusal).expect("read refusal");
            return Err(message.trim_end_matches('\n').to_owned());
        }
        Ok(std::fs::read(self.directory.join(name))
            .unwrap_or_else(|error| panic!("{}/{name}: {error}", self.name)))
    }
}

pub fn cases() -> Vec<Case> {
    let mut found = std::fs::read_dir(golden_root())
        .expect("read tests/golden")
        .map(|entry| {
            entry
                .expect("directory entry")
                .file_name()
                .into_string()
                .expect("UTF-8 case name")
        })
        .collect::<Vec<_>>();
    found.sort();
    assert_eq!(found, CASES, "tests/golden holds exactly the known cases");
    CASES
        .into_iter()
        .map(|name| {
            let directory = golden_root().join(name);
            let case: Value = serde_json::from_str(
                &std::fs::read_to_string(directory.join("case.json")).expect("read case.json"),
            )
            .expect("case.json is JSON");
            Case {
                name,
                input: std::fs::read_to_string(directory.join("input.json"))
                    .expect("read input.json"),
                spec_path: case["spec_path"].as_str().expect("spec_path").to_owned(),
                output: case["output"].as_str().map(str::to_owned),
                spec: case["spec"].as_str().expect("spec").to_owned(),
                directory,
            }
        })
        .collect()
}

pub fn unwired_modes() -> [(&'static str, &'static [&'static str]); 2] {
    [("", &[]), (".unwired", &["--allow-unwired"])]
}

static NEXT_SCRATCH: AtomicU64 = AtomicU64::new(0);

/// A fresh directory under the test target directory.
pub fn scratch(label: &str) -> PathBuf {
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "{label}-{}-{}",
        std::process::id(),
        NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&directory).expect("create scratch directory");
    directory
}

pub fn command() -> Command {
    Command::new(env!("CARGO_BIN_EXE_fsl-testgen"))
}

/// Run `fsl-testgen ARGS` in `cwd` with `stdin`.
pub fn run_in(cwd: &Path, args: &[&str], stdin: &str) -> Output {
    let mut child = command()
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run fsl-testgen");
    let mut pipe = child.stdin.take().expect("stdin");
    let text = stdin.to_owned();
    let writer = std::thread::spawn(move || {
        // The process may exit without reading its input.
        let _ = pipe.write_all(text.as_bytes());
    });
    let output = child.wait_with_output().expect("wait for fsl-testgen");
    writer.join().expect("stdin writer");
    output
}

pub fn run(args: &[&str], stdin: &str) -> Output {
    run_in(&repository_root(), args, stdin)
}

/// The one-line error object a failed run prints on stderr.
pub fn failure(output: &Output) -> (i32, String, String) {
    assert!(
        output.stdout.is_empty(),
        "a failure prints nothing on stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let text = String::from_utf8(output.stderr.clone()).expect("UTF-8 stderr");
    assert_eq!(text.lines().count(), 1, "one line of stderr: {text}");
    let error: Value = serde_json::from_str(&text).expect("stderr is one JSON object");
    assert_eq!(error["result"], "error");
    assert_eq!(error.as_object().expect("object").len(), 3, "{text}");
    (
        output.status.code().expect("exit code"),
        error["kind"].as_str().expect("kind").to_owned(),
        error["message"].as_str().expect("message").to_owned(),
    )
}
