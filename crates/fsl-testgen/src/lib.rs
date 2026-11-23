// SPDX-License-Identifier: Apache-2.0

//! `fsl-testgen`: conformance-test generators for FSL specs. The input is the
//! `testgen-input.v1` document `fslc testgen-input` emits.

pub mod cli;
pub mod plugin_check;
pub mod plugin_host;
pub mod targets;
