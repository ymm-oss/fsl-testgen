// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::fmt::Write as _;

use fsl_testgen_core::render::{python_literal, python_string, unique_ident};
use fsl_testgen_core::{
    GeneratedFile, Options, OutputContext, Target, TestgenInput, UnwiredAdapter,
};
use serde_json::{Map, Value};

use super::one_file;

/// The pytest variant: its adapter is the `Adapter` class, not a factory.
pub(super) const NOT_WIRED_MESSAGE: &str = "adapter not wired: implement Adapter.reset/step/observe to connect your implementation, or generate with `fslc testgen --allow-unwired` to skip instead";

/// pytest scaffold that replays the scenarios and runs the spec's Monitor.
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
            emit(input, options),
        ))
    }
}

fn emit(input: &TestgenInput, options: &Options) -> String {
    let unwired = options.unwired;
    let path_expr = match options.path.output {
        OutputContext::WithoutOutput => python_string(&options.path.relative_spec_path()),
        OutputContext::WithOutput { .. } => format!(
            "Path(__file__).resolve().parent / {}",
            python_string(&options.path.relative_spec_path())
        ),
    };
    let source_name = input.source_name();
    let (unwired_doc, unwired_constant, unwired_action) = match unwired {
        UnwiredAdapter::Fail => (
            "Until Adapter is wired every test FAILS with \"adapter not wired\" (issue #1200);\n\
             generate with `fslc testgen --allow-unwired` to skip instead.\n",
            format!(
                "\nADAPTER_NOT_WIRED = {}\n",
                python_string(NOT_WIRED_MESSAGE)
            ),
            "pytest.fail(ADAPTER_NOT_WIRED, pytrace=False)",
        ),
        UnwiredAdapter::Skip => ("", String::new(), "pytest.skip('Adapter not implemented')"),
    };
    let mut text = format!(
        r#""""Auto-generated conformance tests for FSL spec.
Source: {source_name}
Connect Adapter to your implementation, or use MonitorSelfAdapter for self-check.
{unwired_doc}"""
import random
from pathlib import Path

import pytest

from fslc.runtime import Monitor

SPEC_PATH = {path_expr}
{unwired_constant}

class Adapter:
    """Connect your implementation to the spec actions/state.

    Wiring convention:
    - reset(): put implementation in the same initial state as spec init
    - step(action, params): drive one spec action on the implementation
    - observe(): return implementation state projected to spec logical state shape
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
"#
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
    text.push_str(
        &r#"
def test_random_walk_conformance(adapter):
    if not _adapter_ready(adapter):
        __UNWIRED_ACTION__
    mon = Monitor(SPEC_PATH)
    mon.reset()
    adapter.reset()
    assert adapter.observe() == mon.state
    rng = random.Random(0)
    for _ in range(100):
        enabled = mon.enabled()
        if not enabled:
            break
        choice = enabled[rng.randrange(len(enabled))]
        action, params = choice['action'], dict(choice['params'])
        adapter.step(action, params)
        result = mon.step(action, params)
        if not result.get('ok'):
            pytest.fail(
                f'spec oracle violation at {action} {params}: '
                f"{result.get('kind')} {result.get('name', '')}"
            )
        assert adapter.observe() == mon.state

"#
        .replace("__UNWIRED_ACTION__", unwired_action),
    );
    text
}
