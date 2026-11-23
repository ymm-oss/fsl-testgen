// SPDX-License-Identifier: Apache-2.0

use fsl_testgen_core::render::{fill_template, json_inline, json_rows};
use fsl_testgen_core::{GeneratedFile, Options, Target, TestgenInput, UnwiredAdapter};
use serde_json::Value;

use super::{NOT_WIRED_MESSAGE, fail_header, one_file};

/// Standalone Vitest conformance scaffold.
pub struct Vitest;

impl Target for Vitest {
    fn name(&self) -> &str {
        "vitest"
    }

    fn generate(
        &self,
        input: &TestgenInput,
        options: &Options,
    ) -> Result<Vec<GeneratedFile>, String> {
        validate_integers(input.walk(), "walk")?;
        for (index, scenario) in input.scenarios().iter().enumerate() {
            validate_integers(scenario, &format!("scenarios[{index}]"))?;
        }
        Ok(one_file(
            "conformance.test.ts".to_owned(),
            emit(input, options.unwired),
        ))
    }
}

fn validate_integers(value: &Value, path: &str) -> Result<(), String> {
    const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
    match value {
        Value::Number(number)
            if number
                .as_i64()
                .is_some_and(|value| !(-MAX_SAFE_INTEGER..=MAX_SAFE_INTEGER).contains(&value))
                || number
                    .as_u64()
                    .is_some_and(|value| value > MAX_SAFE_INTEGER.cast_unsigned()) =>
        {
            Err(format!(
                "testgen --target vitest: integer {number} at {path} is outside the exact JavaScript integer range -{MAX_SAFE_INTEGER}..{MAX_SAFE_INTEGER}"
            ))
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                validate_integers(value, &format!("{path}[{index}]"))?;
            }
            Ok(())
        }
        Value::Object(values) => {
            for (key, value) in values {
                validate_integers(value, &format!("{path}.{key}"))?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn scenario_block(scenario: &Value) -> String {
    let name = scenario["name"].as_str().unwrap_or_default();
    let mut lines = vec![
        format!(
            "scenario({}, () => {{",
            json_inline(&Value::String(format!("scenario: {name}")))
        ),
        "  adapter.reset();".to_owned(),
    ];
    let steps = scenario["steps"].as_array().cloned().unwrap_or_default();
    let states = scenario["expected_states"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let empty = Value::Object(serde_json::Map::new());
    for (index, step) in steps.iter().enumerate() {
        lines.push(format!(
            "  adapter.step({}, {});",
            json_inline(&step["action"]),
            json_inline(&step["params"])
        ));
        lines.push(format!(
            "  assertPartial(adapter.observe(), {});",
            json_inline(states.get(index).unwrap_or(&empty))
        ));
    }
    if scenario["kind"].as_str() == Some("forbidden") && scenario["forbidden_step"].is_object() {
        let step = &scenario["forbidden_step"];
        lines.push(format!(
            "  const result = adapter.step({}, {});",
            json_inline(&step["action"]),
            json_inline(&step["params"])
        ));
        lines.push(format!(
            "  assertRejected(result, {});",
            json_inline(&scenario["rejected_by"])
        ));
    }
    lines.push("});".to_owned());
    lines.join("\n")
}

/// The template slots for one unwired-adapter behaviour.
fn unwired_slots(unwired: UnwiredAdapter) -> [(&'static str, String); 3] {
    match unwired {
        UnwiredAdapter::Fail => [
            ("__UNWIRED_HEADER__", fail_header(" * ")),
            (
                "__MAKE_ADAPTER_NOTE__",
                "// Wire your implementation here. Throwing fails every test (\"adapter not wired\")."
                    .to_owned(),
            ),
            (
                "__UNWIRED_GUARD__",
                [
                    format!(
                        "const ADAPTER_NOT_WIRED = {};",
                        json_inline(&Value::String(NOT_WIRED_MESSAGE.to_owned()))
                    ),
                    String::new(),
                    "let adapter: Adapter;".to_owned(),
                    "let unwired: unknown = null;".to_owned(),
                    "try {".to_owned(),
                    "  adapter = makeAdapter();".to_owned(),
                    "  adapter.reset();".to_owned(),
                    "  adapter.observe();".to_owned(),
                    "} catch (error) {".to_owned(),
                    "  unwired = error;".to_owned(),
                    "}".to_owned(),
                    String::new(),
                    "// An unwired adapter fails every test below; it is never skipped silently."
                        .to_owned(),
                    "function scenario(name: string, body: () => void): void {".to_owned(),
                    "  test(name, () => {".to_owned(),
                    "    if (unwired !== null) {".to_owned(),
                    "      throw new Error(`${ADAPTER_NOT_WIRED} (${String(unwired)})`);".to_owned(),
                    "    }".to_owned(),
                    "    body();".to_owned(),
                    "  });".to_owned(),
                    "}".to_owned(),
                ]
                .join("\n"),
            ),
        ],
        UnwiredAdapter::Skip => [
            (
                "__UNWIRED_HEADER__",
                " * Wire `makeAdapter()` to your implementation. Until it is wired, every test is\n \
                 * skipped (mirroring the pytest scaffold's skip-when-unwired behaviour)."
                    .to_owned(),
            ),
            (
                "__MAKE_ADAPTER_NOTE__",
                "// Wire your implementation here. Throwing leaves the suite skipped (not failed)."
                    .to_owned(),
            ),
            (
                "__UNWIRED_GUARD__",
                [
                    "let adapter: Adapter;",
                    "let wired = false;",
                    "try {",
                    "  adapter = makeAdapter();",
                    "  adapter.reset();",
                    "  adapter.observe();",
                    "  wired = true;",
                    "} catch {",
                    "  // Adapter not wired yet — every test below is skipped.",
                    "}",
                    "const scenario = wired ? test : test.skip;",
                ]
                .join("\n"),
            ),
        ],
    }
}

fn emit(input: &TestgenInput, unwired: UnwiredAdapter) -> String {
    let walk = input.walk();
    let slots = unwired_slots(unwired);
    let mut parts = vec![fill_template(
        include_str!("vitest.txt"),
        input.source_name(),
        &slots,
    )];
    parts.extend(input.scenarios().iter().map(scenario_block));
    let steps = walk["steps"].as_array().cloned().unwrap_or_default();
    parts.push(
        [
            "interface WalkStep {".to_owned(),
            "  action: string;".to_owned(),
            "  params: Record<string, unknown>;".to_owned(),
            "  expected: Record<string, unknown>;".to_owned(),
            "}".to_owned(),
            String::new(),
            format!(
                "const RANDOM_WALK_INITIAL: Record<string, unknown> = {};",
                json_inline(&walk["initial"])
            ),
            format!("const RANDOM_WALK: WalkStep[] = {};", json_rows(&steps)),
            String::new(),
            "scenario(\"random-walk conformance (baked oracle trace)\", () => {".to_owned(),
            "  adapter.reset();".to_owned(),
            "  assertPartial(adapter.observe(), RANDOM_WALK_INITIAL);".to_owned(),
            "  for (const step of RANDOM_WALK) {".to_owned(),
            "    adapter.step(step.action, step.params);".to_owned(),
            "    assertPartial(adapter.observe(), step.expected);".to_owned(),
            "  }".to_owned(),
            "});".to_owned(),
        ]
        .join("\n"),
    );
    parts.join("\n\n") + "\n"
}
