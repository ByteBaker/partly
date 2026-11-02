// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

mod models;
mod resolution;
#[cfg(test)]
mod tests;
mod validation;

pub use models::{Resolution, TypeDefinition};
pub use validation::{MAX_STRING_BYTES, valid_type_name, validate_types};
