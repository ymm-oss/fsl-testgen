// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fmt::Write as _;

use fsl_testgen_core::render::{python_literal, python_string, unique_ident};
use fsl_testgen_core::{GeneratedFile, Options, Target, TestgenInput, UnwiredAdapter};
use serde_json::{Map, Value};

use super::one_file;

/// The pytest variant: its adapter is the `Adapter` class, not a factory.
pub(super) const NOT_WIRED_MESSAGE: &str = "adapter not wired: implement Adapter.reset/step/observe to connect your implementation, or generate with `fsl-testgen generate --allow-unwired` to skip instead";

/// Self-contained pytest suite: the scenarios and the walk are baked in, and
/// the generated file imports nothing but pytest.
///
/// It is a different suite from the one `fslc testgen --target pytest`
/// prints, which runs the spec through `fslc.runtime` at test time; see
/// `docs/PYTEST.md` for what that changes.
pub struct Pytest;

impl Target for Pytest {
    fn name(&self) -> &str {
        "pytest"
    }

    fn generate(
        &self,
        input: &TestgenInput,
        options: &Options,
    ) -> Result<Vec<GeneratedFile>, String> {
        Ok(one_file(
            "test_conformance.py".to_owned(),
            emit(input, options.unwired),
        ))
    }
}

const ADAPTER: &str = r#"

class Adapter:
    """Connect your implementation to the spec actions/state.

    Wiring convention:
    - reset(): put implementation in the same initial state as spec init
    - step(action, params): drive one spec action on the implementation;
      return {'ok': False, 'kind': ...} when the implementation rejects it
    - observe(): return implementation state projected to spec logical state shape
      (enum = name string, Option = None|value, Seq = list, Map = dict with string
      keys, struct = dict)
    """

    def reset(self):
        raise NotImplementedError("wire your implementation reset")

    def step(self, action: str, params: dict):
        raise NotImplementedError("wire your implementation step")

    def observe(self) -> dict:
        raise NotImplementedError("wire your implementation observe")


def _adapter_ready(adapter):
    try:
        adapter.reset()
        adapter.observe()
        return True
    except NotImplementedError:
        return False


@pytest.fixture
def adapter():
    return Adapter()


def _assert_partial_expected(observed, expected):
    for key, val in expected.items():
        if isinstance(val, dict) and isinstance(observed.get(key), dict):
            _assert_partial_expected(observed[key], val)
        else:
            assert observed[key] == val


def _assert_rejected(result, expected_kind):
    assert isinstance(result, dict), 'forbidden adapter.step must return a result dict'
    assert result.get('ok') is False
    if expected_kind is not None:
        assert result.get('kind') == expected_kind
"#;

const WALK_TEST: &str = r#"

def test_random_walk_conformance(adapter):
    'random-walk conformance (baked oracle trace)'
    if not _adapter_ready(adapter):
        __UNWIRED_ACTION__
    adapter.reset()
    assert adapter.observe() == RANDOM_WALK_INITIAL, 'initial state'
    for index, (action, params, expected) in enumerate(RANDOM_WALK):
        adapter.step(action, params)
        assert adapter.observe() == expected, f'random walk step {index}: {action} {params}'
"#;

fn emit(input: &TestgenInput, unwired: UnwiredAdapter) -> String {
    let source_name = input.source_name();
    let digest = input.document()["source"]["digest"]
        .as_str()
        .unwrap_or_default();
    let (unwired_doc, unwired_constant, unwired_action) = match unwired {
        UnwiredAdapter::Fail => (
            "Until Adapter is wired every test FAILS with \"adapter not wired\": a suite that\n\
             never calls the implementation is not evidence that it conforms. Generate with\n\
             `fsl-testgen generate --allow-unwired` to skip instead.\n",
            format!(
                "\nADAPTER_NOT_WIRED = {}\n",
                python_string(NOT_WIRED_MESSAGE)
            ),
            "pytest.fail(ADAPTER_NOT_WIRED, pytrace=False)",
        ),
        UnwiredAdapter::Skip => (
            "Until Adapter is wired every test is skipped (generated with --allow-unwired).\n",
            String::new(),
            "pytest.skip('Adapter not implemented')",
        ),
    };
    let mut text = format!(
        r#""""Auto-generated conformance tests for FSL spec.
Source: {source_name}
Source digest: {digest}

Connect Adapter to your implementation.
{unwired_doc}
The expected states below were baked at generation time: the spec's scenarios,
and a walk the FSL Monitor (the spec's concrete interpreter) took under a fixed
seed. This file imports nothing but pytest and reads neither the spec nor
`fslc` when it runs, so it checks the spec as it was when the file was
generated: regenerate it when the spec changes.
"""
import pytest
{unwired_constant}{ADAPTER}"#
    );
    let state_order = input.state_order();
    let mut seen = BTreeMap::new();
    for (scenario_index, scenario) in input.scenarios().iter().enumerate() {
        let name = scenario["name"].as_str().unwrap_or("scenario");
        let function = unique_ident(name, &mut seen, "test_scenario_");
        let separator = if scenario_index == 0 { "\n\n" } else { "\n" };
        let _ = write!(
            text,
            "{separator}def {function}(adapter):\n    {}\n    if not _adapter_ready(adapter):\n        {unwired_action}\n    adapter.reset()\n",
            python_string(&format!("Scenario: {name}"))
        );
        let steps = scenario["steps"].as_array().cloned().unwrap_or_default();
        let states = scenario["expected_states"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for (index, step) in steps.iter().enumerate() {
            let action = step["action"].as_str().unwrap_or_default();
            let _ = writeln!(
                text,
                "    adapter.step({}, {})",
                python_string(action),
                python_literal(&step["params"], input.params_of(action))
            );
            let expected = states
                .get(index)
                .cloned()
                .unwrap_or_else(|| Value::Object(Map::new()));
            let _ = writeln!(
                text,
                "    _assert_partial_expected(adapter.observe(), {})",
                python_literal(&expected, state_order)
            );
        }
        if scenario["kind"].as_str() == Some("forbidden") && scenario["forbidden_step"].is_object()
        {
            let step = &scenario["forbidden_step"];
            let action = step["action"].as_str().unwrap_or_default();
            let _ = writeln!(
                text,
                "    result = adapter.step({}, {})",
                python_string(action),
                python_literal(&step["params"], input.params_of(action))
            );
            let _ = writeln!(
                text,
                "    _assert_rejected(result, {})",
                python_literal(&scenario["rejected_by"], &[])
            );
        }
    }
    // The walk is printed in the walk's own member order, as the other
    // targets print it.
    let walk = input.walk();
    let _ = write!(
        text,
        "\n\nRANDOM_WALK_INITIAL = {}\n",
        python_literal(&walk["initial"], &[])
    );
    let rows = walk["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|step| {
            format!(
                "    ({}, {}, {}),\n",
                python_literal(&step["action"], &[]),
                python_literal(&step["params"], &[]),
                python_literal(&step["expected"], &[])
            )
        })
        .collect::<String>();
    if rows.is_empty() {
        text.push_str("RANDOM_WALK = []\n");
    } else {
        let _ = write!(text, "RANDOM_WALK = [\n{rows}]\n");
    }
    text.push_str(&WALK_TEST.replace("__UNWIRED_ACTION__", unwired_action));
    text
}
