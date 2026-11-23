// SPDX-License-Identifier: Apache-2.0

use fsl_testgen_core::render::{Language, fill_template, literal, quoted, scenario_parts};
use fsl_testgen_core::{GeneratedFile, Options, Target, TestgenInput, UnwiredAdapter};
use serde_json::Value;

use super::forbidden::append_forbidden;
use super::{NOT_WIRED_MESSAGE, fail_header, one_file};

/// Standalone package:test Dart conformance scaffold.
pub struct Dart;

impl Target for Dart {
    fn name(&self) -> &str {
        "dart"
    }

    fn generate(
        &self,
        input: &TestgenInput,
        options: &Options,
    ) -> Result<Vec<GeneratedFile>, String> {
        Ok(one_file(
            "conformance_test.dart".to_owned(),
            emit(input, options.unwired),
        ))
    }
}

/// The template slots and test terminator for one unwired-adapter behaviour.
fn unwired_slots(unwired: UnwiredAdapter) -> ([(&'static str, String); 2], &'static str) {
    match unwired {
        UnwiredAdapter::Fail => (
            [
                ("__UNWIRED_HEADER__", fail_header("// ")),
                (
                    "__MAKE_ADAPTER__",
                    format!(
                        "// Wire your implementation here. Return your adapter instead of throwing.\n\
                         // Until then every test fails with \"adapter not wired\".\n\
                         Adapter makeAdapter() =>\n    throw UnimplementedError({});",
                        quoted(NOT_WIRED_MESSAGE, Language::Dart)
                    ),
                ),
            ],
            "  });",
        ),
        UnwiredAdapter::Skip => (
            [
                (
                    "__UNWIRED_HEADER__",
                    "// Wire `makeAdapter()` to your implementation. Until it is wired every test is\n\
                     // skipped (a top-level probe sets `skip:` on each test), mirroring the other\n\
                     // targets' skip-when-unwired behaviour."
                        .to_owned(),
                ),
                (
                    "__MAKE_ADAPTER__",
                    "// Wire your implementation here. Return your adapter instead of throwing.\n\
                     Adapter makeAdapter() =>\n    \
                     throw UnimplementedError('wire your implementation: implement makeAdapter()');\n\
                     \n\
                     bool _adapterWired() {\n  \
                       try {\n    \
                         final a = makeAdapter();\n    \
                         a.reset();\n    \
                         a.observe();\n    \
                         return true;\n  \
                       } catch (_) {\n    \
                         return false;\n  \
                       }\n\
                     }"
                    .to_owned(),
                ),
            ],
            "  }, skip: wired ? null : 'Adapter not wired');",
        ),
    }
}

fn emit(input: &TestgenInput, unwired: UnwiredAdapter) -> String {
    let walk = input.walk();
    let (slots, test_end) = unwired_slots(unwired);
    let preamble = fill_template(include_str!("dart.txt"), input.source_name(), &slots);
    let mut lines = vec!["void main() {".to_owned()];
    if unwired == UnwiredAdapter::Skip {
        lines.push("  final wired = _adapterWired();".to_owned());
    }
    for scenario in input.scenarios() {
        let (name, steps, states) = scenario_parts(scenario);
        lines.push(String::new());
        lines.push(format!(
            "  test({}, () {{",
            quoted(&format!("scenario: {name}"), Language::Dart)
        ));
        lines.push("    final a = makeAdapter();".to_owned());
        lines.push("    a.reset();".to_owned());
        for (index, step) in steps.iter().enumerate() {
            lines.push(format!(
                "    a.step({}, {});",
                literal(&step["action"], Language::Dart),
                literal(&step["params"], Language::Dart)
            ));
            lines.push(format!(
                "    assertPartial(a.observe(), {});",
                literal(states.get(index).unwrap_or(&Value::Null), Language::Dart)
            ));
        }
        append_forbidden(&mut lines, scenario, Language::Dart, "    ");
        lines.push(test_end.to_owned());
    }
    let rows = walk["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|step| {
            format!(
                "      {{'action': {}, 'params': {}, 'expected': {}}}",
                literal(&step["action"], Language::Dart),
                literal(&step["params"], Language::Dart),
                literal(&step["expected"], Language::Dart)
            )
        })
        .collect::<Vec<_>>();
    let walk_literal = if rows.is_empty() {
        "<Map<String, dynamic>>[]".to_owned()
    } else {
        format!("<Map<String, dynamic>>[\n{},\n    ]", rows.join(",\n"))
    };
    lines.extend([
        String::new(),
        "  test('random-walk conformance (baked oracle trace)', () {".to_owned(),
        "    final a = makeAdapter();".to_owned(),
        "    a.reset();".to_owned(),
        format!(
            "    final initial = {};",
            literal(&walk["initial"], Language::Dart)
        ),
        "    assertPartial(a.observe(), Map<String, dynamic>.from(initial));".to_owned(),
        format!("    final walk = {walk_literal};"),
        "    for (final step in walk) {".to_owned(),
        "      a.step(step['action'] as String, step['params'] as Map<String, dynamic>);"
            .to_owned(),
        "      assertPartial(a.observe(), step['expected'] as Map<String, dynamic>);".to_owned(),
        "    }".to_owned(),
        test_end.to_owned(),
        "}".to_owned(),
    ]);
    format!("{preamble}\n\n{}\n", lines.join("\n"))
}
