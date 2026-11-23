// SPDX-License-Identifier: Apache-2.0

//! The vendored `testgen-input.v1` schema accepts the golden inputs.

mod support;

use serde_json::Value;
use support::{cases, repository_root};

fn schema(name: &str) -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(repository_root().join("schemas").join(name))
            .unwrap_or_else(|error| panic!("read {name}: {error}")),
    )
    .unwrap_or_else(|error| panic!("{name} is not JSON: {error}"))
}

fn validator() -> jsonschema::Validator {
    let mut registry = jsonschema::Registry::new();
    let referenced = [
        "testgen-trace.v1.schema.json",
        "kernel.v1.schema.json",
        "test-plan.v1.schema.json",
    ]
    .map(schema);
    for resource in &referenced {
        registry = registry
            .add(resource["$id"].as_str().expect("$id"), resource)
            .expect("schema resource");
    }
    let registry = registry.prepare().expect("schema registry");
    jsonschema::options()
        .with_registry(&registry)
        .build(&schema("testgen-input.v1.schema.json"))
        .expect("testgen-input schema compiles")
}

fn schema_errors(validator: &jsonschema::Validator, document: &Value) -> Vec<String> {
    validator
        .iter_errors(document)
        .map(|error| format!("{}: {error}", error.instance_path()))
        .collect()
}

/// Cases whose document does not validate, and why.
///
/// Known upstream defect (ymm-oss/fsl): for governance and ai-component specs
/// the embedded Public Kernel v1 carries spans with a zero line or column
/// (`end_line: 0`, `end_column: 0` in the golden), which `kernel.v1` forbids
/// (`minimum: 1`); `fslc kernel` prints the same spans. Such a document still passes the reader's cross-checks and
/// reproduces `fslc testgen`. The exception is exact: the document must fail, and must validate once
/// those zeros are replaced, so the day the Kernel is fixed this test says to
/// remove the exception.
const KERNEL_BREAKS_ITS_SCHEMA: [&str; 1] = ["governance"];

#[test]
fn golden_inputs_validate_against_the_vendored_schema() {
    let validator = validator();
    let mut excepted = Vec::new();
    for case in cases() {
        let errors = schema_errors(&validator, &case.document());
        if KERNEL_BREAKS_ITS_SCHEMA.contains(&case.name) {
            assert!(
                !errors.is_empty(),
                "{}: the Kernel now validates; remove the exception",
                case.name
            );
            let mut repaired = case.document();
            // The same document validates once its spans are legal.
            fn fix_spans(value: &mut Value) {
                match value {
                    Value::Object(object) => {
                        for (key, member) in object.iter_mut() {
                            if matches!(key.as_str(), "line" | "column" | "end_line" | "end_column")
                                && *member == 0
                            {
                                *member = Value::from(1);
                            } else {
                                fix_spans(member);
                            }
                        }
                    }
                    Value::Array(items) => items.iter_mut().for_each(fix_spans),
                    _ => {}
                }
            }
            fix_spans(&mut repaired["kernel"]);
            assert_eq!(
                schema_errors(&validator, &repaired),
                Vec::<String>::new(),
                "{}: zero spans in the Kernel are the only violation",
                case.name
            );
            excepted.push(case.name);
            continue;
        }
        assert_eq!(errors, Vec::<String>::new(), "{}", case.name);
    }
    assert_eq!(excepted, KERNEL_BREAKS_ITS_SCHEMA);
}

/// The schema is closed where the reader is: a document the reader rejects
/// for an unknown member does not validate either.
#[test]
fn the_schema_rejects_what_it_should() {
    let validator = validator();
    let case = cases()
        .into_iter()
        .find(|case| case.name == "kernel")
        .expect("kernel case");
    let mut unknown = case.document();
    unknown["surprise"] = Value::from(1);
    assert!(!schema_errors(&validator, &unknown).is_empty());
    let mut no_kernel = case.document();
    no_kernel["kernel"] = Value::Null;
    assert!(!schema_errors(&validator, &no_kernel).is_empty());
}
