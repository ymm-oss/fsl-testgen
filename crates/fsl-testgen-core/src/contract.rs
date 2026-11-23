// SPDX-License-Identifier: Apache-2.0

//! Identities of the contracts `testgen-input.v1` is made of, and the helpers
//! that read a Public Kernel v1 document. Copied from ymm-oss/fsl
//! (`fsl-core` and `fsl-tools`), which defines them.

use serde_json::{Map, Value};

pub const KERNEL_SCHEMA_VERSION: &str = "1.0.0";
pub const KERNEL_SCHEMA_ID: &str = "https://fsl.dev/schemas/fslc/kernel/kernel.v1.schema.json";
pub const TESTGEN_TRACE_V1_SCHEMA_VERSION: &str = "1.0.0";
pub const TESTGEN_TRACE_V1_SCHEMA_ID: &str =
    "https://fsl.dev/schemas/fslc/kernel/testgen-trace.v1.schema.json";
pub const TESTGEN_INPUT_V1_SCHEMA_VERSION: &str = "1.0.0";
pub const TESTGEN_INPUT_V1_SCHEMA_ID: &str =
    "https://fsl.dev/schemas/fslc/kernel/testgen-input.v1.schema.json";
pub const TEST_PLAN_V1_SCHEMA_VERSION: &str = "1.0.0";
pub const TEST_PLAN_V1_SCHEMA_ID: &str =
    "https://fsl.dev/schemas/fslc/kernel/test-plan.v1.schema.json";

/// The database dialect's stand-in for a public `__` inside a generated
/// name, restored when the name is published.
const DB_SEPARATOR: &str = "QqDbSepqQ";

/// Public spelling of a Kernel identifier: the internal-to-public string rule
/// of the Public Kernel (`alias__name` is published as `alias.name`).
#[must_use]
pub fn unmapped_public_spelling(name: &str) -> String {
    name.replacen("__", ".", 1).replace(DB_SEPARATOR, "__")
}

pub(crate) fn required_object<'a>(
    value: &'a Value,
    context: &str,
) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| format!("public Kernel {context} must be an object"))
}

pub(crate) fn required_array<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<&'a [Value], String> {
    object
        .get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("public Kernel {context}.{key} must be an array"))
}

pub(crate) fn required_str<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    context: &str,
) -> Result<&'a str, String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("public Kernel {context}.{key} must be a string"))
}

pub(crate) fn public_kernel_v1_root(kernel: &Value) -> Result<&Map<String, Value>, String> {
    let root = required_object(kernel, "root")?;
    let schema = required_str(root, "$schema", "root")?;
    if schema != KERNEL_SCHEMA_ID {
        return Err(format!(
            "unsupported public Kernel $schema '{schema}'; expected '{KERNEL_SCHEMA_ID}'"
        ));
    }
    let version = required_str(root, "schema_version", "root")?;
    if version != KERNEL_SCHEMA_VERSION {
        return Err(format!(
            "unsupported public Kernel schema_version '{version}'; expected '{KERNEL_SCHEMA_VERSION}'"
        ));
    }
    Ok(root)
}
