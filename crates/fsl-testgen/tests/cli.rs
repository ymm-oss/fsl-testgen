// SPDX-License-Identifier: Apache-2.0

//! Exit codes and the error shape of the command line (see `src/cli.rs`).

mod support;

use serde_json::{Value, json};
use support::{TARGETS, cases, failure, run, scratch};

fn kernel_input() -> String {
    cases()
        .into_iter()
        .find(|case| case.name == "kernel")
        .expect("kernel case")
        .input
}

fn mutated(mutate: impl FnOnce(&mut Value)) -> String {
    let mut document: Value = serde_json::from_str(&kernel_input()).expect("JSON");
    mutate(&mut document);
    serde_json::to_string(&document).expect("serialize")
}

#[test]
fn targets_lists_the_built_in_targets() {
    let done = run(&["targets"], "");
    assert_eq!(done.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(done.stdout).expect("UTF-8"),
        TARGETS.map(|target| format!("{target}\n")).concat()
    );
}

#[test]
fn usage_errors_exit_2() {
    for (args, needle) in [
        (&[][..], "usage: fsl-testgen generate"),
        (&["frobnicate"][..], "unknown command 'frobnicate'"),
        (&["generate"][..], "--target is required"),
        (&["generate", "--target"][..], "--target needs a value"),
        (
            &["generate", "--target", "pytest", "--target", "dart"][..],
            "--target is given more than once",
        ),
        (
            &["generate", "--target", "pytest", "--depth", "3"][..],
            "unknown argument '--depth'",
        ),
        (
            &["generate", "--target", "cobol"][..],
            "unknown target 'cobol'; expected pytest, vitest, swift, kotlin, dart, phpunit",
        ),
        (&["targets", "extra"][..], "unknown argument 'extra'"),
    ] {
        let (code, kind, message) = failure(&run(args, &kernel_input()));
        assert_eq!((code, kind.as_str()), (2, "usage"), "{args:?}");
        assert!(message.contains(needle), "{args:?}: {message}");
    }
}

#[test]
fn a_rejected_document_exits_3_and_writes_nothing() {
    let output = scratch("rejected").join("out.py");
    let output_argument = output.to_str().expect("UTF-8 path");
    for (input, expected) in [
        (
            "{".to_owned(),
            "testgen-input is not JSON: EOF while parsing an object at line 1 column 1",
        ),
        (
            mutated(|d| d["schema_version"] = json!("2.0.0")),
            "unsupported testgen-input schema_version '2.0.0'; expected 1.x",
        ),
        (
            mutated(|d| d["surprise"] = json!(1)),
            "testgen-input has unknown field 'surprise'",
        ),
        (
            mutated(|d| d["walk"]["steps"][0]["action"] = json!("ghost")),
            "testgen concrete vector.steps[0].action names unknown action 'ghost'",
        ),
        (
            mutated(|d| d["scenarios"].as_array_mut().expect("scenarios").reverse()),
            "testgen-input.scenarios is not normalized to the order of state and actions",
        ),
    ] {
        let done = run(
            &["generate", "--target", "pytest", "-o", output_argument],
            &input,
        );
        assert_eq!(failure(&done), (3, "input".to_owned(), expected.to_owned()));
        assert!(!output.exists(), "nothing is written for {expected}");
    }
}

#[test]
fn a_later_minor_is_accepted_and_another_major_is_not() {
    let minor = mutated(|d| d["schema_version"] = json!("1.7.2"));
    let done = run(&["generate", "--target", "dart"], &minor);
    assert_eq!(done.status.code(), Some(0));
    assert_eq!(
        done.stdout,
        run(&["generate", "--target", "dart"], &kernel_input()).stdout
    );
    for version in ["0.9.0", "2.0.0", "1.0", "one"] {
        let done = run(
            &["generate", "--target", "dart"],
            &mutated(|d| d["schema_version"] = json!(version)),
        );
        let (code, kind, message) = failure(&done);
        assert_eq!((code, kind.as_str()), (3, "input"), "{version}");
        assert!(message.contains("schema_version"), "{message}");
    }
}

#[test]
fn a_refusing_target_exits_4() {
    let done = run(
        &["generate", "--target", "kotlin", "--allow-unwired"],
        &kernel_input(),
    );
    let (code, kind, message) = failure(&done);
    assert_eq!((code, kind.as_str()), (4, "target"));
    assert!(message.starts_with("--allow-unwired is not available for --target kotlin"));
}

#[test]
fn unreadable_input_and_unwritable_output_exit_5() {
    let directory = scratch("io");
    let missing = directory.join("missing.json");
    let done = run(
        &[
            "generate",
            "--target",
            "pytest",
            "--input",
            missing.to_str().expect("UTF-8 path"),
        ],
        "",
    );
    let (code, kind, message) = failure(&done);
    assert_eq!((code, kind.as_str()), (5, "io"));
    assert!(message.starts_with("cannot read "), "{message}");

    let nowhere = directory.join("no/such/directory/out.py");
    let done = run(
        &[
            "generate",
            "--target",
            "pytest",
            "-o",
            nowhere.to_str().expect("UTF-8 path"),
        ],
        &kernel_input(),
    );
    let (code, kind, message) = failure(&done);
    assert_eq!((code, kind.as_str()), (5, "io"));
    assert!(message.starts_with("cannot write "), "{message}");
    assert!(!nowhere.exists());
}

#[test]
fn input_is_read_from_a_file_or_stdin_alike() {
    let directory = scratch("input-file");
    let file = directory.join("input.json");
    std::fs::write(&file, kernel_input()).expect("write input");
    let from_file = run(
        &[
            "generate",
            "--target",
            "swift",
            "--input",
            file.to_str().expect("UTF-8 path"),
        ],
        "",
    );
    let from_stdin = run(&["generate", "--target", "swift"], &kernel_input());
    assert_eq!(from_file.status.code(), Some(0));
    assert_eq!(from_file.stdout, from_stdin.stdout);
    assert!(!from_file.stdout.is_empty());
}

/// `-o` leaves either the whole file or nothing, and no temporary file.
#[test]
fn output_is_written_whole_and_leaves_no_temporary_file() {
    let directory = scratch("atomic");
    let output = directory.join("conformance.test.ts");
    std::fs::write(&output, "previous").expect("write previous content");
    let done = run(
        &[
            "generate",
            "--target",
            "vitest",
            "-o",
            output.to_str().expect("UTF-8 path"),
        ],
        &kernel_input(),
    );
    assert_eq!(done.status.code(), Some(0));
    assert_eq!(
        std::fs::read(&output).expect("read output"),
        run(&["generate", "--target", "vitest"], &kernel_input()).stdout
    );
    let names = std::fs::read_dir(&directory)
        .expect("read directory")
        .map(|entry| entry.expect("entry").file_name())
        .collect::<Vec<_>>();
    assert_eq!(names, ["conformance.test.ts"]);
}
