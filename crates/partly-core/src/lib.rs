// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

//! Policy validation and local evaluation shared by all Partly SDKs.

mod evaluation;
mod models;
mod numbers;
#[cfg(feature = "openapi")]
mod openapi;
mod policy;
mod rules;
#[cfg(test)]
mod tests;

pub use evaluation::bucket;
pub use models::{
    Condition, Context, Decision, EvaluationError, Flag, MAX_POLICY_BYTES, MatchAction, Operator,
    Policy,
};
pub use numbers::validate_json_number_limits;
pub use policy::valid_key;
pub use rules::value_type;
