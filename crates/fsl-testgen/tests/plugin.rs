// SPDX-License-Identifier: Apache-2.0

//! External targets (plugin protocol 1): how one is found, what it is sent,
//! and every way its answer is refused. Each refusal has its own exit code
//! and message, and writes nothing.
//!
//! The misbehaving plugins are shell scripts written into a scratch
//! directory; the well-behaved one is `examples/unittest-plugin`.

#![cfg(unix)]

mod support;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use support::{cases, failure, repository_root, run_in, run_with, scratch};

fn golden(name: &str) -> String {
    cases()
        .into_iter()
        .find(|case| case.name == name)
        .expect("golden case")
        .input
}

/// A project directory with an `fsl-testgen.toml`, an output directory, and
/// a place plugins can leave evidence that they ran.
struct Project {
    root: PathBuf,
    out: PathBuf,
}

impl Project {
    fn new(label: &str) -> Self {
        let root = scratch(label);
        let out = root.join("out");
        std::fs::create_dir(&out).expect("create out");
        Self { root, out }
    }

    /// Write an executable shell script; its path.
    fn script(&self, name: &str, body: &str) -> PathBuf {
        let path = self.root.join(name);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create parent");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("write script");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("make executable");
        path
    }

    /// A plugin that reads its request and prints `response`.
    fn answering(&self, name: &str, response: &Value) -> PathBuf {
        let text = response.to_string();
        assert!(!text.contains('\''), "the response is quoted for sh");
        self.script(name, &format!("cat >/dev/null\nprintf '%s' '{text}'"))
    }

    /// Map target `name` to `command` in the project's configuration.
    fn configure(&self, name: &str, command: &[&str], extra: &str) {
        std::fs::write(
            self.root.join("fsl-testgen.toml"),
            format!("[targets.{name}]\ncommand = {}\n{extra}\n", json!(command)),
        )
        .expect("write configuration");
    }

    fn generate(&self, target: &str, extra: &[&str], input: &str) -> Output {
        let mut args = vec!["generate", "--target", target];
        args.extend(extra);
        run_in(&self.root, &args, input)
    }

    fn generate_into_out(&self, target: &str, input: &str) -> Output {
        self.generate(target, &["--out-dir", "out"], input)
    }

    /// Every file and directory under the project, relative, sorted.
    fn tree(&self) -> Vec<String> {
        fn walk(directory: &Path, root: &Path, found: &mut Vec<String>) {
            for entry in std::fs::read_dir(directory).expect("read directory") {
                let path = entry.expect("entry").path();
                found.push(
                    path.strip_prefix(root)
                        .expect("under root")
                        .to_string_lossy()
                        .into_owned(),
                );
                if path.is_dir() && !path.is_symlink() {
                    walk(&path, root, found);
                }
            }
        }
        let mut found = Vec::new();
        walk(&self.root, &self.root, &mut found);
        found.sort();
        found
    }
}

fn response(files: &[(&str, &str)]) -> Value {
    json!({
        "protocol": 1,
        "files": files
            .iter()
            .map(|(path, content)| json!({"path": path, "content": content}))
            .collect::<Vec<_>>(),
        "diagnostics": [],
    })
}

// ---- Finding the plugin -------------------------------------------------------

#[test]
fn a_configured_command_wins_over_an_executable_on_path() {
    let project = Project::new("plugin-resolve");
    let configured = project.answering("tools/configured", &response(&[("t.txt", "configured\n")]));
    project.answering(
        "bin/fsl-testgen-target-demo",
        &response(&[("t.txt", "from PATH\n")]),
    );
    let path = format!(
        "{}:{}",
        project.root.join("bin").display(),
        std::env::var("PATH").expect("PATH")
    );
    let generate = |directory: &Path| {
        run_with(
            directory,
            &["generate", "--target", "demo"],
            &golden("kernel"),
            &[("PATH", &path)],
        )
    };

    // No configuration: the executable on PATH.
    let done = generate(&project.root);
    assert_eq!(done.status.code(), Some(0), "{done:?}");
    assert_eq!(done.stdout, b"from PATH\n");

    // The configuration is found from a directory below it, and wins. Its
    // relative command is resolved from the configuration's directory.
    project.configure("demo", &["tools/configured"], "");
    let below = project.root.join("deep/er");
    std::fs::create_dir_all(&below).expect("create directories");
    assert!(configured.exists());
    let done = generate(&below);
    assert_eq!(done.status.code(), Some(0), "{done:?}");
    assert_eq!(done.stdout, b"configured\n");

    // A target neither names is unknown, and says where it looked.
    let done = run_with(
        &project.root,
        &["generate", "--target", "nope"],
        &golden("kernel"),
        &[("PATH", &path)],
    );
    assert_eq!(
        failure(&done),
        (
            2,
            "usage".to_owned(),
            "unknown target 'nope'; expected pytest, vitest, swift, kotlin, dart, phpunit, a [targets.nope] entry in fsl-testgen.toml, or fsl-testgen-target-nope on PATH".to_owned()
        )
    );
}

#[test]
fn a_built_in_target_cannot_be_shadowed() {
    let project = Project::new("plugin-shadow");
    let marker = project.root.join("ran");
    let impostor = project.script(
        "bin/fsl-testgen-target-pytest",
        &format!(
            "touch '{}'\ncat >/dev/null\nprintf '%s' '{}'",
            marker.display(),
            response(&[("t", "impostor")])
        ),
    );
    let path = format!(
        "{}:{}",
        project.root.join("bin").display(),
        std::env::var("PATH").expect("PATH")
    );
    // An executable named after a built-in target is never consulted.
    let done = run_with(
        &project.root,
        &["generate", "--target", "pytest"],
        &golden("kernel"),
        &[("PATH", &path)],
    );
    assert_eq!(done.status.code(), Some(0));
    assert!(
        String::from_utf8_lossy(&done.stdout).starts_with("\"\"\"Auto-generated conformance tests")
    );
    assert!(!marker.exists(), "the impostor ran");

    // A configuration entry named after one is refused, whatever the target.
    project.configure("pytest", &[impostor.to_str().expect("UTF-8 path")], "");
    for target in ["pytest", "vitest", "other"] {
        let done = project.generate_into_out(target, &golden("kernel"));
        let (code, kind, message) = failure(&done);
        assert_eq!((code, kind.as_str()), (7, "config"), "{target}");
        assert!(
            message.ends_with(
                "fsl-testgen.toml: [targets.pytest] names a built-in target; built-in targets cannot be replaced"
            ),
            "{message}"
        );
    }
    assert!(!marker.exists(), "the impostor ran");
    assert_eq!(std::fs::read_dir(&project.out).expect("out").count(), 0);
}

// ---- What the plugin is sent ---------------------------------------------------

fn request_validator() -> jsonschema::Validator {
    let schema = |name: &str| -> Value {
        serde_json::from_str(
            &std::fs::read_to_string(repository_root().join("schemas").join(name))
                .expect("read schema"),
        )
        .expect("schema is JSON")
    };
    let referenced = [
        "testgen-input.v1.schema.json",
        "testgen-trace.v1.schema.json",
        "kernel.v1.schema.json",
        "test-plan.v1.schema.json",
    ]
    .map(schema);
    let mut registry = jsonschema::Registry::new();
    for resource in &referenced {
        registry = registry
            .add(resource["$id"].as_str().expect("$id"), resource)
            .expect("schema resource");
    }
    let registry = registry.prepare().expect("schema registry");
    jsonschema::options()
        .with_registry(&registry)
        .build(&schema("plugin-request.v1.schema.json"))
        .expect("request schema compiles")
}

#[test]
fn the_request_is_the_document_and_the_options() {
    let project = Project::new("plugin-request");
    let seen = project.root.join("request.json");
    let plugin = project.script(
        "recorder",
        &format!(
            "cat > '{}'\nprintf '%s' '{}'",
            seen.display(),
            response(&[("t.txt", "x")])
        ),
    );
    project.configure("recorder", &[plugin.to_str().expect("UTF-8 path")], "");
    let validator = request_validator();
    for (case, flags, options) in [
        (
            "compose",
            &[][..],
            json!({"unwired": "fail", "spec_path": null, "output": null}),
        ),
        (
            "kernel",
            &[
                "--allow-unwired",
                "--spec-path",
                "specs/cart.fsl",
                "--out-dir",
                "out",
            ][..],
            json!({"unwired": "skip", "spec_path": "specs/cart.fsl", "output": "out"}),
        ),
        (
            "forbidden",
            &["-o", "out/one.txt"][..],
            json!({"unwired": "fail", "spec_path": null, "output": "out/one.txt"}),
        ),
    ] {
        let input = golden(case);
        let done = project.generate("recorder", flags, &input);
        assert_eq!(done.status.code(), Some(0), "{case}: {done:?}");
        let text = std::fs::read_to_string(&seen).expect("the plugin saw a request");
        assert!(text.ends_with("}\n"), "one line");
        assert_eq!(text.lines().count(), 1);
        let request: Value = serde_json::from_str(&text).expect("the request is JSON");
        assert_eq!(
            validator
                .iter_errors(&request)
                .map(|error| error.to_string())
                .collect::<Vec<_>>(),
            Vec::<String>::new(),
            "{case}"
        );
        assert_eq!(request["protocol"], 1);
        assert_eq!(request["options"], options, "{case}");
        // The document is passed on as given, member order included.
        let document: Value = serde_json::from_str(&input).expect("golden is JSON");
        assert_eq!(
            serde_json::to_string(&request["input"]).expect("serialize"),
            serde_json::to_string(&document).expect("serialize"),
            "{case}"
        );
    }
}

/// The document is checked before the plugin is started: a plugin never
/// receives one that failed a cross-check.
#[test]
fn a_rejected_document_never_reaches_the_plugin() {
    let project = Project::new("plugin-rejected-input");
    let marker = project.root.join("ran");
    let plugin = project.script(
        "eager",
        &format!(
            "touch '{}'\ncat >/dev/null\nprintf '%s' '{}'",
            marker.display(),
            response(&[("t.txt", "x")])
        ),
    );
    project.configure("eager", &[plugin.to_str().expect("UTF-8 path")], "");
    let mut document: Value = serde_json::from_str(&golden("kernel")).expect("JSON");
    for (mutate, expected) in [
        (
            Box::new(|d: &mut Value| d["walk"]["steps"][0]["action"] = json!("ghost"))
                as Box<dyn Fn(&mut Value)>,
            "testgen concrete vector.steps[0].action names unknown action 'ghost'",
        ),
        (
            Box::new(|d: &mut Value| d["schema_version"] = json!("2.0.0")),
            "unsupported testgen-input schema_version '2.0.0'; expected 1.x",
        ),
        (
            Box::new(|d: &mut Value| d["scenarios"].as_array_mut().expect("array").reverse()),
            "testgen-input.scenarios is not normalized to the order of state and actions",
        ),
    ] {
        let original = document.clone();
        mutate(&mut document);
        let done = project.generate_into_out("eager", &document.to_string());
        assert_eq!(failure(&done), (3, "input".to_owned(), expected.to_owned()));
        assert!(!marker.exists(), "the plugin was started for: {expected}");
        document = original;
    }
    let done = project.generate_into_out("eager", "not json");
    assert_eq!(failure(&done).0, 3);
    assert!(!marker.exists());
    assert_eq!(std::fs::read_dir(&project.out).expect("out").count(), 0);
    // The same plugin does run for a good document.
    let done = project.generate_into_out("eager", &golden("kernel"));
    assert_eq!(done.status.code(), Some(0));
    assert!(marker.exists());
}

// ---- Refusals: each for its own reason, nothing written ------------------------

/// Run the configured target `bad` into `out` and expect a refusal of `kind`
/// whose message contains `needle`, with the project left as it was.
fn refused(project: &Project, extra: &[&str], kind: (i32, &str), needle: &str) {
    let before = project.tree();
    let mut args = vec!["--out-dir", "out"];
    args.extend(extra);
    let done = project.generate("bad", &args, &golden("kernel"));
    let (code, name, message) = failure(&done);
    assert_eq!((code, name.as_str()), kind, "{message}");
    assert!(message.contains(needle), "{message}");
    assert_eq!(project.tree(), before, "something was written: {message}");
}

#[test]
fn a_plugin_that_writes_outside_is_refused() {
    let project = Project::new("plugin-outside");
    for (path, reason) in [
        ("../evil.txt", "has a '..' component"),
        ("sub/../../evil.txt", "has a '..' component"),
        ("/tmp/fsl-testgen-evil.txt", "is absolute"),
        ("C:/evil.txt", "starts with a drive letter"),
        ("sub\\\\..\\\\evil.txt", "contains a backslash"),
        ("./evil.txt", "has a '.' component"),
        ("sub//evil.txt", "has an empty component"),
        ("", "is empty"),
    ] {
        // A good file first: it must not be written either.
        let plugin = project.answering(
            "bad",
            &response(&[("good.txt", "good\n"), (path, "evil\n")]),
        );
        project.configure("bad", &[plugin.to_str().expect("UTF-8 path")], "");
        refused(&project, &[], (6, "plugin"), reason);
        refused(&project, &[], (6, "plugin"), "response.files[1].path");
    }
    assert!(
        !project
            .root
            .parent()
            .expect("parent")
            .join("evil.txt")
            .exists()
    );
    assert!(!Path::new("/tmp/fsl-testgen-evil.txt").exists());
    assert_eq!(std::fs::read_dir(&project.out).expect("out").count(), 0);
}

/// A relative path can still leave the output directory through a symbolic
/// link that is already there. That is refused when writing.
#[test]
fn a_path_that_leaves_through_a_symbolic_link_is_refused() {
    let project = Project::new("plugin-symlink");
    let outside = project.root.join("outside");
    std::fs::create_dir(&outside).expect("create outside");
    std::os::unix::fs::symlink(&outside, project.out.join("link")).expect("create link");
    let plugin = project.answering(
        "bad",
        &response(&[("new/good.txt", "good\n"), ("link/evil.txt", "evil\n")]),
    );
    project.configure("bad", &[plugin.to_str().expect("UTF-8 path")], "");
    refused(
        &project,
        &[],
        (5, "io"),
        "generated path 'link/evil.txt' leaves out through a symbolic link",
    );
    assert_eq!(std::fs::read_dir(&outside).expect("outside").count(), 0);
    assert!(
        !project.out.join("new").exists(),
        "a created directory is removed again"
    );
}

#[test]
fn a_malformed_response_is_refused() {
    let project = Project::new("plugin-malformed");
    for (body, needle) in [
        (
            "cat >/dev/null\nprintf 'generated 3 tests'",
            "the response is not JSON",
        ),
        ("cat >/dev/null", "the response is not JSON"),
        ("cat >/dev/null\nprintf '[]'", "response must be an object"),
        (
            "cat >/dev/null\nprintf '{\"protocol\": 2, \"files\": [], \"diagnostics\": []}'",
            "response.protocol is 2; this fsl-testgen speaks protocol 1",
        ),
        (
            "cat >/dev/null\nprintf '{\"protocol\": 1, \"files\": []}'",
            "response.diagnostics is required",
        ),
        (
            "cat >/dev/null\nprintf '{\"protocol\": 1, \"files\": [{\"path\": \"a\", \"content\": \"x\", \"mode\": 493}], \"diagnostics\": []}'",
            "response.files[0] has unknown member 'mode'",
        ),
        (
            "cat >/dev/null\nprintf '{\"protocol\": 1, \"files\": [{\"path\": \"a\", \"content\": \"x\"}, {\"path\": \"a\", \"content\": \"y\"}], \"diagnostics\": []}'",
            "response.files[1].path 'a' is given more than once",
        ),
        (
            "cat >/dev/null\nprintf '\\377\\376'",
            "printed text that is not UTF-8",
        ),
    ] {
        let plugin = project.script("bad", body);
        project.configure("bad", &[plugin.to_str().expect("UTF-8 path")], "");
        refused(&project, &[], (6, "plugin"), needle);
    }
}

#[test]
fn a_plugin_that_exits_non_zero_is_refused_whatever_it_printed() {
    let project = Project::new("plugin-exit");
    let plugin = project.script(
        "bad",
        &format!(
            "cat >/dev/null\nprintf '%s' '{}'\necho 'boom: no such template' >&2\nexit 3",
            response(&[("good.txt", "good\n")])
        ),
    );
    project.configure("bad", &[plugin.to_str().expect("UTF-8 path")], "");
    refused(
        &project,
        &[],
        (6, "plugin"),
        "exited with status 3; stderr: boom: no such template",
    );

    let missing = project.root.join("no-such-plugin");
    project.configure("bad", &[missing.to_str().expect("UTF-8 path")], "");
    refused(
        &project,
        &[],
        (6, "plugin"),
        "cannot start plugin [targets.bad]",
    );
}

#[test]
fn a_plugin_that_hangs_is_stopped() {
    let project = Project::new("plugin-hang");
    // `exec`, so that the process that is stopped is the one that sleeps.
    let plugin = project.script("bad", "exec sleep 600");
    project.configure("bad", &[plugin.to_str().expect("UTF-8 path")], "");
    let started = Instant::now();
    refused(
        &project,
        &["--plugin-timeout", "1"],
        (6, "plugin"),
        "did not finish within 1 s and was stopped",
    );
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "{:?}",
        started.elapsed()
    );

    // The configuration can set the limit, and the flag overrides it.
    project.configure(
        "bad",
        &[plugin.to_str().expect("UTF-8 path")],
        "timeout_seconds = 1",
    );
    let started = Instant::now();
    let before = project.tree();
    let done = project.generate_into_out("bad", &golden("kernel"));
    let (code, kind, message) = failure(&done);
    assert_eq!((code, kind.as_str()), (6, "plugin"));
    assert!(message.contains("did not finish within 1 s"), "{message}");
    assert!(started.elapsed() < Duration::from_secs(20));
    assert_eq!(project.tree(), before);

    // A slow plugin that answers after the limit is stopped too, although its
    // answer would have been valid.
    let slow = project.script(
        "bad",
        &format!(
            "cat >/dev/null\nsleep 3\nprintf '%s' '{}'",
            response(&[("late.txt", "late\n")])
        ),
    );
    project.configure("bad", &[slow.to_str().expect("UTF-8 path")], "");
    refused(
        &project,
        &["--plugin-timeout", "1"],
        (6, "plugin"),
        "did not finish within 1 s",
    );
    // Given the time, the same plugin succeeds.
    let done = project.generate(
        "bad",
        &["--out-dir", "out", "--plugin-timeout", "30"],
        &golden("kernel"),
    );
    assert_eq!(done.status.code(), Some(0), "{done:?}");
    assert_eq!(
        std::fs::read_to_string(project.out.join("late.txt")).expect("read"),
        "late\n"
    );
}

#[test]
fn an_error_diagnostic_refuses_and_a_warning_is_passed_on() {
    let project = Project::new("plugin-diagnostics");
    let plugin = project.answering(
        "bad",
        &json!({
            "protocol": 1,
            "files": [{"path": "good.txt", "content": "good\n"}],
            "diagnostics": [
                {"severity": "warning", "message": "w"},
                {"severity": "error", "message": "this target cannot skip at run time"},
                {"severity": "error", "message": "second"}
            ],
        }),
    );
    project.configure("bad", &[plugin.to_str().expect("UTF-8 path")], "");
    refused(
        &project,
        &[],
        (4, "target"),
        "this target cannot skip at run time; second",
    );

    let plugin = project.answering(
        "bad",
        &json!({
            "protocol": 1,
            "files": [{"path": "good.txt", "content": "good\n"}],
            "diagnostics": [
                {"severity": "warning", "message": "state field x is not asserted"},
                {"severity": "info", "message": "1 file"}
            ],
        }),
    );
    project.configure("bad", &[plugin.to_str().expect("UTF-8 path")], "");
    let done = project.generate_into_out("bad", &golden("kernel"));
    assert_eq!(done.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(done.stderr).expect("UTF-8"),
        "{\"result\":\"diagnostic\",\"severity\":\"warning\",\"message\":\"state field x is not asserted\"}\n{\"result\":\"diagnostic\",\"severity\":\"info\",\"message\":\"1 file\"}\n"
    );
    assert_eq!(
        std::fs::read_to_string(project.out.join("good.txt")).expect("read"),
        "good\n"
    );
}

// ---- Writing --------------------------------------------------------------------

#[test]
fn several_files_are_written_under_out_dir_or_not_at_all() {
    let project = Project::new("plugin-files");
    let plugin = project.answering(
        "bad",
        &response(&[
            ("a.txt", "A\n"),
            ("deep/er/b.txt", "B\n"),
            ("deep/c.txt", "C\n"),
        ]),
    );
    project.configure("bad", &[plugin.to_str().expect("UTF-8 path")], "");

    // Several files cannot go to stdout or to one -o file.
    for extra in [&[][..], &["-o", "out/one.txt"][..]] {
        let before = project.tree();
        let done = project.generate("bad", extra, &golden("kernel"));
        assert_eq!(
            failure(&done),
            (
                2,
                "usage".to_owned(),
                "target 'bad' generated 3 files; use --out-dir DIR to write them".to_owned()
            )
        );
        assert_eq!(project.tree(), before);
    }

    // One file that cannot be written (its path is an existing directory)
    // stops all of them, and the directories created on the way are removed.
    std::fs::create_dir_all(project.out.join("deep/c.txt")).expect("create obstacle");
    refused(
        &project,
        &[],
        (5, "io"),
        "generated path 'deep/c.txt' is an existing directory",
    );
    std::fs::remove_dir(project.out.join("deep/c.txt")).expect("remove obstacle");
    std::fs::remove_dir(project.out.join("deep")).expect("remove obstacle");

    // An output directory that does not exist is not created.
    let done = project.generate("bad", &["--out-dir", "missing"], &golden("kernel"));
    let (code, kind, message) = failure(&done);
    assert_eq!((code, kind.as_str()), (5, "io"));
    assert!(
        message.starts_with("cannot write into missing: "),
        "{message}"
    );
    assert!(!project.root.join("missing").exists());

    std::fs::write(project.out.join("a.txt"), "previous").expect("write previous");
    let done = project.generate_into_out("bad", &golden("kernel"));
    assert_eq!(done.status.code(), Some(0), "{done:?}");
    let report: Value = serde_json::from_slice(&done.stdout).expect("report");
    assert_eq!(report["result"], "generated");
    assert_eq!(report["target"], "bad");
    assert_eq!(
        report["files"],
        json!(["out/a.txt", "out/deep/er/b.txt", "out/deep/c.txt"])
    );
    assert_eq!(
        project
            .tree()
            .into_iter()
            .filter(|path| path.starts_with("out/"))
            .collect::<Vec<_>>(),
        [
            "out/a.txt",
            "out/deep",
            "out/deep/c.txt",
            "out/deep/er",
            "out/deep/er/b.txt"
        ],
        "no temporary file is left"
    );
    assert_eq!(
        std::fs::read_to_string(project.out.join("a.txt")).expect("read"),
        "A\n"
    );
    assert_eq!(
        std::fs::read_to_string(project.out.join("deep/er/b.txt")).expect("read"),
        "B\n"
    );

    // A built-in target goes through the same writer, under its own file name.
    let done = project.generate_into_out("kotlin", &golden("kernel"));
    assert_eq!(done.status.code(), Some(0), "{done:?}");
    assert!(project.out.join("ShoppingCartConformanceTest.kt").is_file());
}

// ---- The published response schema and the reader agree -------------------------

#[test]
fn the_response_schema_and_the_reader_agree() {
    let schema: Value = serde_json::from_str(
        &std::fs::read_to_string(repository_root().join("schemas/plugin-response.v1.schema.json"))
            .expect("read schema"),
    )
    .expect("schema is JSON");
    let validator = jsonschema::validator_for(&schema).expect("response schema compiles");
    let file = |path: &str| json!({"protocol": 1, "files": [{"path": path, "content": ""}], "diagnostics": []});
    let mut samples = vec![
        (response(&[]), true),
        (response(&[("a/b.c", "x"), ("d", "")]), true),
        (
            json!({"protocol": 1, "files": [], "diagnostics": [{"severity": "info", "message": ""}]}),
            true,
        ),
        (
            json!({"protocol": 1, "files": [], "diagnostics": [{"severity": "fatal", "message": ""}]}),
            false,
        ),
        (
            json!({"protocol": 1, "files": [], "diagnostics": [{"severity": "error"}]}),
            false,
        ),
        (
            json!({"protocol": 2, "files": [], "diagnostics": []}),
            false,
        ),
        (
            json!({"protocol": "1", "files": [], "diagnostics": []}),
            false,
        ),
        (json!({"protocol": 1, "files": []}), false),
        (
            json!({"protocol": 1, "files": [], "diagnostics": [], "extra": true}),
            false,
        ),
        (
            json!({"protocol": 1, "files": [{"path": "a"}], "diagnostics": []}),
            false,
        ),
        (
            json!({"protocol": 1, "files": [{"path": "a", "content": 1}], "diagnostics": []}),
            false,
        ),
        (
            json!({"protocol": 1, "files": [{"path": "a", "content": "", "mode": 1}], "diagnostics": []}),
            false,
        ),
        (json!([]), false),
    ];
    for path in ["a", "a/b", "..a/b..", "a/...", "a.b/c-d", "a b"] {
        samples.push((file(path), true));
    }
    for path in [
        "", "/a", "C:/a", "c:a", "..", "../a", "a/../b", "a/..", ".", "./a", "a/./b", "a//b", "a/",
        "a\\b", "a\u{0}b",
    ] {
        samples.push((file(path), false));
    }
    for (sample, valid) in samples {
        let text = sample.to_string();
        assert_eq!(validator.is_valid(&sample), valid, "schema: {text}");
        assert_eq!(
            fsl_testgen_core::plugin::parse_response(&text).is_ok(),
            valid,
            "reader: {text}"
        );
    }
}

// ---- plugin-check ------------------------------------------------------------------

fn plugin_check(project: &Project, command: &[&str]) -> (i32, String) {
    let mut args = vec!["plugin-check"];
    args.extend(command);
    let done = run_in(&project.root, &args, "");
    (
        done.status.code().expect("exit code"),
        String::from_utf8(done.stdout).expect("UTF-8 report"),
    )
}

fn example_plugin() -> String {
    repository_root()
        .join("examples/unittest-plugin/fsl-testgen-target-unittest")
        .to_str()
        .expect("UTF-8 path")
        .to_owned()
}

#[test]
fn plugin_check_passes_the_example_plugin_and_says_what_it_did_not_check() {
    let project = Project::new("plugin-check-example");
    let (code, report) = plugin_check(&project, &[&example_plugin()]);
    assert_eq!(code, 0, "{report}");
    assert!(
        report.contains("\nplugin-check: 24 checks, 0 failed\n"),
        "{report}"
    );
    assert!(!report.contains("FAIL  "), "{report}");
    for line in [
        "ok    compose (unwired=fail), a compose input with kernel: null: valid response, the same on a second run, 1 file(s)",
        "ok    escaped-name (unwired=skip): valid response",
        "ok    governance (unwired=fail): valid response",
        "note  escaped-name (unwired=fail): the source name \"we'ird*na>me?.fsl\" is printed verbatim in 1 file(s)",
    ] {
        assert!(report.contains(line), "{line}\n{report}");
    }
    for kind in [
        "acceptance",
        "action_coverage",
        "deadlock",
        "forbidden",
        "leadsTo",
        "reachable",
    ] {
        assert!(
            report.contains(&format!("ok    scenario kind {kind} (from ")),
            "{kind}\n{report}"
        );
    }
    // The limits are part of every report.
    assert!(report.contains("Not checked (plugin-check cannot see these):"));
    assert!(report.contains("that the generated suite FAILS while its adapter is not wired"));
    assert!(report.contains("ymm-oss/fsl issue #1200"));
}

#[test]
fn plugin_check_fails_a_plugin_for_what_it_can_see() {
    let project = Project::new("plugin-check-bad");

    // The same answer whatever the request: valid and deterministic, but the
    // scenarios make no difference to it.
    let constant = project.answering("constant", &response(&[("t.txt", "one test\n")]));
    let (code, report) = plugin_check(&project, &[constant.to_str().expect("UTF-8 path")]);
    assert_eq!(code, 1, "{report}");
    assert!(
        report.contains("plugin-check: 24 checks, 6 failed"),
        "{report}"
    );
    assert!(
        report.contains("FAIL  scenario kind forbidden (from requirements): the output is the same as for a document without scenarios"),
        "{report}"
    );
    assert!(
        report.contains("ok    kernel (unwired=fail): valid response"),
        "{report}"
    );

    // A different answer on every run.
    let random = project.script(
        "random",
        "cat >/dev/null\nprintf '{\"protocol\": 1, \"files\": [{\"path\": \"t.txt\", \"content\": \"%s\"}], \"diagnostics\": []}' \"$$\"",
    );
    let (code, report) = plugin_check(&project, &[random.to_str().expect("UTF-8 path")]);
    assert_eq!(code, 1);
    assert!(
        report.contains(
            "FAIL  kernel (unwired=fail): not deterministic: two runs on the same request differ"
        ),
        "{report}"
    );

    // A path that escapes, an empty file, no file, a refusal, a crash.
    for (body, needle) in [
        (
            format!("cat >/dev/null\nprintf '%s' '{}'", response(&[("../t.txt", "x")])),
            "FAIL  kernel (unwired=fail): plugin",
        ),
        (
            format!("cat >/dev/null\nprintf '%s' '{}'", response(&[("t.txt", "")])),
            "FAIL  kernel (unwired=fail): 't.txt' is empty",
        ),
        (
            format!("cat >/dev/null\nprintf '%s' '{}'", response(&[])),
            "FAIL  kernel (unwired=fail): no files and no error",
        ),
        (
            "cat >/dev/null\nprintf '{\"protocol\": 1, \"files\": [], \"diagnostics\": [{\"severity\": \"error\", \"message\": \"no\"}]}'".to_owned(),
            "FAIL  kernel (unwired=fail): the plugin refuses a golden input: no",
        ),
        (
            "exit 9".to_owned(),
            "exited with status 9",
        ),
    ] {
        let plugin = project.script("bad", &body);
        let (code, report) = plugin_check(&project, &[plugin.to_str().expect("UTF-8 path")]);
        assert_eq!(code, 1, "{report}");
        assert!(report.contains(needle), "{needle}\n{report}");
    }
    // Refusing only the skipping mode is an answer, not a failure.
    let refuses_skip = project.script(
        "strict",
        &format!(
            "if grep -q '\"unwired\":\"skip\"'; then printf '%s' '{}'; else printf '%s' '{}'; fi",
            json!({"protocol": 1, "files": [], "diagnostics": [{"severity": "error", "message": "cannot skip at run time"}]}),
            response(&[("t.txt", "x")])
        ),
    );
    let (_, report) = plugin_check(&project, &[refuses_skip.to_str().expect("UTF-8 path")]);
    assert!(
        report.contains("note  kernel (unwired=skip): refused: cannot skip at run time"),
        "{report}"
    );
    assert!(!report.contains("FAIL  kernel"), "{report}");

    // Usage.
    for args in [
        &["plugin-check"][..],
        &["plugin-check", "--timeout", "x", "cmd"][..],
    ] {
        let done = run_in(&project.root, args, "");
        assert_eq!(failure(&done).0, 2, "{args:?}");
    }
    let (code, report) = plugin_check(
        &project,
        &[
            "--timeout",
            "1",
            project
                .script("bad", "exec sleep 600")
                .to_str()
                .expect("path"),
        ],
    );
    assert_eq!(code, 1);
    assert!(report.contains("did not finish within 1 s"), "{report}");
}

// ---- The example plugin, end to end --------------------------------------------------

/// The requirement `plugin-check` cannot check, shown for the example plugin
/// by running what it generates: unwired, the suite fails; with
/// `--allow-unwired` it is skipped, not passed.
#[test]
fn the_example_plugin_generates_a_suite_that_fails_while_unwired() {
    let examples = repository_root().join("examples/unittest-plugin");
    let python = std::env::var("FSL_TESTGEN_PYTHON").unwrap_or_else(|_| "python3".to_owned());
    for (flags, code, needle) in [
        (&[][..], 1, "FAILED (failures=5)"),
        (&["--allow-unwired"][..], 0, "OK (skipped=5)"),
    ] {
        let out = scratch("plugin-example");
        let mut args = vec![
            "generate",
            "--target",
            "unittest",
            "--out-dir",
            out.to_str().expect("UTF-8 path"),
        ];
        args.extend(flags);
        // The configuration in the example directory maps the target.
        let done = run_in(&examples, &args, &golden("compose"));
        assert_eq!(done.status.code(), Some(0), "{done:?}");
        let suite = out.join("test_banksystem_conformance.py");
        assert!(suite.is_file());
        let run = Command::new(&python)
            .args(["-m", "unittest", "test_banksystem_conformance"])
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .current_dir(&out)
            .output()
            .expect("run unittest");
        let text = String::from_utf8_lossy(&run.stderr).into_owned();
        assert_eq!(run.status.code(), Some(code), "{text}");
        assert!(text.contains(needle), "{text}");
        assert_eq!(
            text.contains("adapter not wired: implement make_adapter()"),
            code == 1
        );
    }
}
