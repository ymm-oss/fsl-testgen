// SPDX-License-Identifier: Apache-2.0

use fsl_testgen_core::contract::{
    KERNEL_SCHEMA_VERSION, TESTGEN_INPUT_V1_SCHEMA_ID, TESTGEN_INPUT_V1_SCHEMA_VERSION,
    TESTGEN_TRACE_V1_SCHEMA_ID, TESTGEN_TRACE_V1_SCHEMA_VERSION,
};
use std::path::PathBuf;

use fsl_testgen_core::{Options, TestgenInput, UnwiredAdapter};
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

fn options(unwired: UnwiredAdapter) -> Options {
    Options {
        unwired,
        ..Options::default()
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

/// Every built-in suite is self-contained: where the spec and the output
/// are does not change a byte of it.
#[test]
fn no_built_in_target_observes_the_paths() {
    let document = document();
    let elsewhere = Options {
        unwired: UnwiredAdapter::Fail,
        spec_path: Some(PathBuf::from("/workspace/first/demo.fsl")),
        output: Some(PathBuf::from("/workspace/generated/out")),
    };
    for target in builtin_targets() {
        assert_eq!(
            emit(&document, target.name(), &options(UnwiredAdapter::Fail)).expect("emit"),
            emit(&document, target.name(), &elsewhere).expect("emit elsewhere"),
            "{}",
            target.name()
        );
    }
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
        let fail =
            emit(&document, target, &options(UnwiredAdapter::default())).expect("emit default");
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
        let skip = emit(&document, target, &options(UnwiredAdapter::Skip));
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
    let output = emit(&document, "pytest", &options(UnwiredAdapter::Fail)).expect("emit pytest");
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
        emit(&document, "vitest", &options(UnwiredAdapter::Fail)).expect_err("out of range"),
        "testgen --target vitest: integer 9007199254740992 at walk.initial.zeta is outside the exact JavaScript integer range -9007199254740991..9007199254740991"
    );
}

/// The pytest suite is baked: it replays the document's walk, compares whole
/// states, asserts a forbidden step as rejected, and imports only pytest.
#[test]
fn pytest_bakes_the_walk_and_imports_only_pytest() {
    let mut document = document();
    document["scenarios"] = json!([{
        "name": "forbidden_begin",
        "kind": "forbidden",
        "action": "begin",
        "initial_state": {"zeta": 0, "alpha": 0},
        "steps": [{"action": "finish", "params": {}}],
        "expected_states": [{"zeta": 0, "alpha": 1}],
        "forbidden_step": {"action": "begin", "params": {"id": 7}},
        "rejected_by": "requires_failed"
    }]);
    // The walk's own member order (`alpha` first) is not `state` order.
    document["walk"]["initial"] = json!({"alpha": 0, "zeta": 0});
    document["walk"]["steps"] = json!([
        {"action": "begin", "params": {"id": 1}, "expected": {"alpha": 0, "zeta": 1}},
        {"action": "finish", "params": {}, "expected": {"alpha": 1, "zeta": 1}}
    ]);
    let output = emit(&document, "pytest", &options(UnwiredAdapter::Fail)).expect("emit pytest");
    let imports = output
        .lines()
        .filter(|line| line.starts_with("import ") || line.starts_with("from "))
        .collect::<Vec<_>>();
    assert_eq!(imports, ["import pytest"]);
    assert!(output.contains(
        "Source digest: sha256:0000000000000000000000000000000000000000000000000000000000000000\n"
    ));
    assert!(output.contains(
        "    adapter.step('finish', {})\n    _assert_partial_expected(adapter.observe(), {'zeta': 0, 'alpha': 1})\n    result = adapter.step('begin', {'id': 7})\n    _assert_rejected(result, 'requires_failed')\n"
    ));
    assert!(output.contains(
        "RANDOM_WALK_INITIAL = {'alpha': 0, 'zeta': 0}\nRANDOM_WALK = [\n    ('begin', {'id': 1}, {'alpha': 0, 'zeta': 1}),\n    ('finish', {}, {'alpha': 1, 'zeta': 1}),\n]\n"
    ));
    assert!(
        output.contains("    assert adapter.observe() == RANDOM_WALK_INITIAL, 'initial state'\n")
    );
    assert!(output.contains(
        "        assert adapter.observe() == expected, f'random walk step {index}: {action} {params}'\n"
    ));
    for gone in ["fslc.runtime", "Monitor(", "SPEC_PATH", "import random"] {
        assert!(!output.contains(gone), "{gone} is still generated");
    }
    // Without walk steps the walk test still compares the initial state.
    let empty = emit(&self::document(), "pytest", &options(UnwiredAdapter::Skip)).expect("emit");
    assert!(empty.contains("RANDOM_WALK = []\n"));
    assert!(empty.contains("pytest.skip('Adapter not implemented')"));
    assert!(!empty.contains("pytest.fail("));
}
