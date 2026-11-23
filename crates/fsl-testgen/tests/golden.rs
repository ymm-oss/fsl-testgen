// SPDX-License-Identifier: Apache-2.0

//! The checked-in goldens. For the frozen targets (vitest, swift, kotlin,
//! dart, phpunit) `fsl-testgen generate` on `input.json` prints the bytes
//! `fslc testgen` printed for the same spec, with and without
//! `--allow-unwired`. pytest is baked here and no longer follows fslc; its
//! goldens are this generator's own output and only detect change. The
//! goldens are only read here; `scripts/regen_goldens.py` writes them.

mod support;

use support::{Case, TARGETS, cases, failure, run, scratch, unwired_modes};

/// Generate `target` for `case` the way the golden was generated: to stdout,
/// or with `-o`.
fn generate(case: &Case, target: &str, flags: &[&str]) -> Result<Vec<u8>, String> {
    let mut args = vec!["generate", "--target", target];
    args.extend(flags);
    let Some(output) = &case.output else {
        let done = run(&args, &case.input);
        if done.status.code() == Some(0) {
            assert!(done.stderr.is_empty(), "{}", case.name);
            return Ok(done.stdout);
        }
        let (code, kind, message) = failure(&done);
        assert_eq!((code, kind.as_str()), (4, "target"), "{}", case.name);
        return Err(message);
    };
    let written = scratch("golden-output").join(output);
    std::fs::create_dir_all(written.parent().expect("output parent")).expect("create parent");
    args.extend(["-o", written.to_str().expect("UTF-8 path")]);
    let done = run(&args, &case.input);
    if done.status.code() == Some(0) {
        let report: serde_json::Value =
            serde_json::from_slice(&done.stdout).expect("a generated report");
        assert_eq!(report["result"], "generated");
        assert_eq!(report["target"], target);
        assert_eq!(report["output"], written.to_str().expect("UTF-8 path"));
        return Ok(std::fs::read(&written).expect("read the written file"));
    }
    let (code, kind, message) = failure(&done);
    assert_eq!((code, kind.as_str()), (4, "target"), "{}", case.name);
    assert!(!written.exists(), "a refusal writes no file");
    Err(message)
}

#[test]
fn every_target_reproduces_the_golden_bytes() {
    let mut compared = 0;
    let mut differences = Vec::new();
    for case in cases() {
        for target in TARGETS {
            for (suffix, flags) in unwired_modes() {
                let name = format!("{target}{suffix}");
                let expected = case.expected(&name);
                let actual = generate(&case, target, flags);
                compared += 1;
                if actual != expected {
                    differences.push(format!("{}/{name}", case.name));
                }
            }
        }
    }
    assert_eq!(compared, 10 * 6 * 2);
    assert_eq!(differences, Vec::<String>::new(), "files that differ");
}

/// Kotlin has no runtime skip: `--allow-unwired` is refused with the message
/// `fslc testgen` gives, for every case.
#[test]
fn kotlin_refuses_allow_unwired_with_the_same_message() {
    for case in cases() {
        assert_eq!(
            case.expected("kotlin.unwired"),
            Err(
                "--allow-unwired is not available for --target kotlin: kotlin.test has no portable runtime skip, so an unwired test would pass without calling the implementation"
                    .to_owned()
            ),
            "{}",
            case.name
        );
        for target in TARGETS {
            for (suffix, _) in unwired_modes() {
                let refused = case.expected(&format!("{target}{suffix}")).is_err();
                assert_eq!(
                    refused,
                    target == "kotlin" && suffix == ".unwired",
                    "{}/{target}{suffix}",
                    case.name
                );
            }
        }
    }
}

/// The compose golden is the case that needs an order-preserving parser: its
/// `walk.initial` is not in `state` order, and not in alphabetical order
/// either, so a parser that sorts or re-keys object members cannot reproduce
/// the bytes above.
#[test]
fn the_compose_golden_exercises_member_order() {
    let case = cases()
        .into_iter()
        .find(|case| case.name == "compose")
        .expect("compose case");
    let document = case.document();
    assert!(document["kernel"].is_null());
    let state = document["state"]
        .as_array()
        .expect("state")
        .iter()
        .map(|name| name.as_str().expect("name").to_owned())
        .collect::<Vec<_>>();
    let walk = document["walk"]["initial"]
        .as_object()
        .expect("walk.initial")
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    assert_ne!(walk, state, "walk order differs from state order");
    let mut sorted = walk.clone();
    sorted.sort();
    assert_ne!(walk, sorted, "walk order is not alphabetical");
    assert_ne!(state, sorted, "state order is not alphabetical");
    // The generated walk is printed in the walk's own order.
    let vitest = String::from_utf8(generate(&case, "vitest", &[]).expect("vitest")).expect("UTF-8");
    let line = vitest
        .lines()
        .find(|line| line.starts_with("const RANDOM_WALK_INITIAL"))
        .expect("the walk's initial state");
    let positions = walk
        .iter()
        .map(|key| line.find(&format!("\"{key}\":")).expect("key is printed"))
        .collect::<Vec<_>>();
    assert!(positions.is_sorted(), "{line}");
}

/// No generated file imports `fslc`, and the pytest suite imports nothing but
/// pytest: the suites run without the spec and without any part of fslc.
/// The goldens are what `generate` prints (the test above), so checking them
/// checks the generator.
#[test]
fn no_generated_file_imports_fslc() {
    let mut checked = 0;
    for case in cases() {
        for target in TARGETS {
            for (suffix, _) in unwired_modes() {
                let Ok(bytes) = case.expected(&format!("{target}{suffix}")) else {
                    continue;
                };
                let text = String::from_utf8(bytes).expect("UTF-8 golden");
                let label = format!("{}/{target}{suffix}", case.name);
                assert!(!text.contains("fslc.runtime"), "{label}");
                assert!(!text.contains("fslc import"), "{label}");
                for line in text.lines() {
                    let statement = line.trim_start();
                    let imports = ["import ", "from ", "use ", "require", "include"]
                        .iter()
                        .any(|keyword| statement.starts_with(keyword));
                    assert!(!(imports && statement.contains("fslc")), "{label}: {line}");
                }
                if target == "pytest" {
                    let imports = text
                        .lines()
                        .filter(|line| line.starts_with("import ") || line.starts_with("from "))
                        .collect::<Vec<_>>();
                    assert_eq!(imports, ["import pytest"], "{label}");
                    for gone in ["SPEC_PATH", "Monitor(", "Path(", "open("] {
                        assert!(!text.contains(gone), "{label}: {gone}");
                    }
                }
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 10 * 11);
}
