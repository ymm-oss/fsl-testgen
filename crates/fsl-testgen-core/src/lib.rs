// SPDX-License-Identifier: Apache-2.0

//! The core of `fsl-testgen`: reading `testgen-input.v1` with every
//! cross-check, the helpers targets print with, and the [`Target`] interface.

pub mod contract;
mod input;
pub mod render;
mod target;
mod test_plan;

pub use input::{Action, TestgenInput, parse_document};
pub use target::{GeneratedFile, Options, Target, UnwiredAdapter};
pub use test_plan::validate_test_plan_v1;
