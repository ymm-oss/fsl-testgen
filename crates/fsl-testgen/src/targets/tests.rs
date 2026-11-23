// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use fsl_testgen_core::contract::{
    KERNEL_SCHEMA_VERSION, TESTGEN_INPUT_V1_SCHEMA_ID, TESTGEN_INPUT_V1_SCHEMA_VERSION,
    TESTGEN_TRACE_V1_SCHEMA_ID, TESTGEN_TRACE_V1_SCHEMA_VERSION,
};
use fsl_testgen_core::{Options, OutputContext, PathContext, TestgenInput, UnwiredAdapter};
use serde_json::{Value, json};

use super::{NOT_WIRED_MESSAGE, builtin_target, builtin_targets, kotlin, pytest};

/// A compose document (no Kernel) without scenarios or walk steps.
fn document() -> Value {
    json!({
        "$schema": TESTGEN_INPUT_V1_SCHEMA_ID,
        "schema_version": TESTGEN_INPUT_V1_SCHEMA_VERSION,
        "kernel_schema_version": KERNEL_SCHEMA_VERSION,
        "result": "testgen_input",
        "fslc_version": "0.0.0",
        "spec": "Demo",
        "source": {
            "name": "demo.fsl",
            "digest": "sha256:0000000000000000000000000000000000000000000000000000000000000000"
        },
        "dialect": "compose",
        "gate": {"depth": 8, "deadlock": "warn", "strict": false},
        "state": ["zeta", "alpha"],
        "actions": [
            {"name": "begin", "params": ["id"]},
            {"name": "finish", "params": []}
        ],
        "scenarios": [],
        "walk": {
            "$schema": TESTGEN_TRACE_V1_SCHEMA_ID,
            "schema_version": TESTGEN_TRACE_V1_SCHEMA_VERSION,
            "kernel_schema_version": KERNEL_SCHEMA_VERSION,
            "result": "testgen_trace",
            "spec": "Demo",
            "initial": {"zeta": 0, "alpha": 0},
            "steps": []
        },
        "kernel": null
    })
}

fn input(document: &Value) -> TestgenInput {
    TestgenInput::from_document(document).expect("read the document")
}

fn options(unwired: UnwiredAdapter, path: PathContext) -> Options {
    Options { unwired, path }
}

fn stdout_path() -> PathContext {
    PathContext {
        spec_path: PathBuf::from("demo.fsl"),
        output: OutputContext::WithoutOutput,
    }
}

fn emit(document: &Value, target: &str, options: &Options) -> Result<String, String> {
    let files = builtin_target(target)
        .expect("built-in target")
        .generate(&input(document), options)?;
    assert_eq!(files.len(), 1, "{target} generates one file");
    Ok(files[0].content.clone())
}

#[test]
fn the_six_targets_are_listed_in_order() {
    assert_eq!(
        builtin_targets()
            .iter()
            .map(|target| target.name().to_owned())
            .collect::<Vec<_>>(),
        ["pytest", "vitest", "swift", "kotlin", "dart", "phpunit"]
    );
    assert!(builtin_target("unknown").is_none());
}

#[test]
fn only_pytest_observes_the_path_context() {
    let document = document();
    let context = |spec: &str| PathContext {
        spec_path: PathBuf::from(spec),
        output: OutputContext::WithOutput {
            parent: Some(PathBuf::from("/workspace/generated")),
        },
    };
    let first = options(UnwiredAdapter::Fail, context("/workspace/first/demo.fsl"));
    let second = options(UnwiredAdapter::Fail, context("/workspace/second/demo.fsl"));
    let first_pytest = emit(&document, "pytest", &first).expect("emit first pytest");
    let second_pytest = emit(&document, "pytest", &second).expect("emit second pytest");
    assert!(
        first_pytest.contains("SPEC_PATH = Path(__file__).resolve().parent / '../first/demo.fsl'")
    );
    assert!(
        second_pytest
            .contains("SPEC_PATH = Path(__file__).resolve().parent / '../second/demo.fsl'")
    );
    for target in ["vitest", "swift", "kotlin", "dart", "phpunit"] {
        assert_eq!(
            emit(&document, target, &first).expect("emit first"),
            emit(&document, target, &second).expect("emit second"),
            "{target} must not observe path-context changes"
        );
    }
    let stdout = emit(
        &document,
        "pytest",
        &options(UnwiredAdapter::Fail, stdout_path()),
    )
    .expect("emit");
    assert!(stdout.contains("Source: demo.fsl"));
    assert!(stdout.contains("SPEC_PATH = 'demo.fsl'"));
}

/// ymm-oss/fsl issue #1200: an unwired generated test fails by default on
/// every target, and only an explicit `UnwiredAdapter::Skip` restores the
/// skip. The behaviour is fixed here on the generated text: the default
/// carries the failing construct and none of the skipping ones.
#[test]
fn unwired_adapter_fails_by_default_and_skips_only_on_opt_in() {
    let document = document();
    let cases: [(&str, &[&str], &[&str]); 6] = [
        (
            "pytest",
            &[
                "pytest.fail(ADAPTER_NOT_WIRED, pytrace=False)",
                pytest::NOT_WIRED_MESSAGE,
            ],
            &["pytest.skip("],
        ),
        (
            "vitest",
            &[
                "throw new Error(`${ADAPTER_NOT_WIRED} (${String(unwired)})`);",
                NOT_WIRED_MESSAGE,
            ],
            &["test.skip", "wired ? test"],
        ),
        (
            "swift",
            &[
                "@Test func ",
                "var description: String {",
                NOT_WIRED_MESSAGE,
            ],
            &[".enabled(if:"],
        ),
        (
            "kotlin",
            &[
                "val a = makeAdapter() ?: fail(ADAPTER_NOT_WIRED)",
                "import kotlin.test.fail",
                kotlin::NOT_WIRED_MESSAGE,
            ],
            &["?: return"],
        ),
        (
            "dart",
            &["throw UnimplementedError('adapter not wired: ", "  });"],
            &["skip:", "_adapterWired"],
        ),
        (
            "phpunit",
            &["$this->fail('adapter not wired: "],
            &["markTestSkipped"],
        ),
    ];
    for (target, present, absent) in cases {
        let fail = emit(
            &document,
            target,
            &options(UnwiredAdapter::default(), stdout_path()),
        )
        .expect("emit default");
        for needle in present {
            assert!(
                fail.contains(needle),
                "{target} default lacks {needle:?}:\n{fail}"
            );
        }
        for needle in absent {
            assert!(
                !fail.contains(needle),
                "{target} default still has {needle:?}:\n{fail}"
            );
        }
        let skip = emit(
            &document,
            target,
            &options(UnwiredAdapter::Skip, stdout_path()),
        );
        if target == "kotlin" {
            let error = skip.expect_err("kotlin has no runtime skip");
            assert!(error.contains("--allow-unwired is not available for --target kotlin"));
            continue;
        }
        let skip = skip.expect("emit skip");
        for needle in absent {
            assert!(
                skip.contains(needle),
                "{target} opt-in lacks {needle:?}:\n{skip}"
            );
        }
        assert!(
            !skip.contains("adapter not wired: implement"),
            "{target} opt-in still fails"
        );
    }
}

#[test]
fn pytest_sanitizes_scenario_names_without_losing_display_names() {
    let mut document = document();
    let scenario = |name: &str| {
        json!({
            "name": name,
            "initial_state": {"zeta": 0, "alpha": 0},
            "steps": [],
            "expected_states": []
        })
    };
    document["scenarios"] = json!([
        scenario("reach_bank.Settled"),
        scenario("reach_bank-Settled")
    ]);
    let output = emit(
        &document,
        "pytest",
        &options(UnwiredAdapter::Fail, stdout_path()),
    )
    .expect("emit pytest");
    assert!(output.contains("def test_scenario_reach_bank_Settled(adapter):"));
    assert!(output.contains("def test_scenario_reach_bank_Settled_2(adapter):"));
    assert!(output.contains("'Scenario: reach_bank.Settled'"));
    assert!(output.contains("'Scenario: reach_bank-Settled'"));
}

#[test]
fn vitest_rejects_integers_javascript_cannot_represent() {
    let mut document = document();
    document["walk"]["initial"]["zeta"] = json!(9_007_199_254_740_992_i64);
    assert_eq!(
        emit(
            &document,
            "vitest",
            &options(UnwiredAdapter::Fail, stdout_path())
        )
        .expect_err("out of range"),
        "testgen --target vitest: integer 9007199254740992 at walk.initial.zeta is outside the exact JavaScript integer range -9007199254740991..9007199254740991"
    );
}
