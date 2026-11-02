// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use super::TypeDefinition;
use crate::Flag;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_STRING_BYTES: usize = 4096;
const MAX_INTEGER: i64 = 9_007_199_254_740_991;
const BUILTINS: [&str; 4] = ["boolean", "string", "integer", "float"];

pub fn valid_type_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name.as_bytes()[0].is_ascii_alphabetic()
        && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
        && ![
            "boolean",
            "string",
            "integer",
            "float",
            "number",
            "object",
            "named",
            "constructor",
            "prototype",
        ]
        .contains(&name)
}

pub fn validate_types(types: &BTreeMap<String, TypeDefinition>) -> Result<(), String> {
    for (name, definition) in types {
        if !valid_type_name(name) {
            return Err("Invalid or reserved result type name".into());
        }
        match definition {
            TypeDefinition::Enum { values } => {
                let distinct: BTreeSet<_> = values.iter().collect();
                if values.is_empty()
                    || values.len() > 64
                    || distinct.len() != values.len()
                    || values.iter().any(|v| v.is_empty() || v.len() > 256)
                {
                    return Err("Enums need 1–64 distinct choices of 1–256 UTF-8 bytes".into());
                }
            }
            TypeDefinition::Record { fields } => {
                if fields.is_empty() || fields.len() > 32 {
                    return Err("Records need 1–32 required fields".into());
                }
                for (field, kind) in fields {
                    if !valid_type_name(field) && !BUILTINS.contains(&field.as_str()) {
                        return Err("Invalid record field name".into());
                    }
                    if !BUILTINS.contains(&kind.as_str())
                        && !matches!(types.get(kind), Some(TypeDefinition::Enum { .. }))
                    {
                        return Err(
                            "Record fields must use a built-in type or an existing enum".into()
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

pub(super) fn kind<'a>(
    name: &'a str,
    types: &BTreeMap<String, TypeDefinition>,
) -> Result<&'a str, String> {
    if BUILTINS.contains(&name) {
        return Ok(name);
    }
    match types.get(name) {
        Some(TypeDefinition::Enum { .. }) => Ok("string"),
        Some(TypeDefinition::Record { .. }) => Ok("object"),
        None => Err("Referenced result type is not defined".into()),
    }
}

fn validate_value(
    value: &Value,
    name: &str,
    types: &BTreeMap<String, TypeDefinition>,
) -> Result<(), String> {
    let valid = match name {
        "boolean" => value.is_boolean(),
        "string" => value.as_str().is_some_and(|s| s.len() <= MAX_STRING_BYTES),
        "integer" => value
            .as_i64()
            .is_some_and(|v| (-MAX_INTEGER..=MAX_INTEGER).contains(&v)),
        "float" => value.as_f64().is_some_and(f64::is_finite),
        _ => match types.get(name) {
            Some(TypeDefinition::Enum { values }) => value
                .as_str()
                .is_some_and(|v| values.iter().any(|choice| choice == v)),
            Some(TypeDefinition::Record { fields }) => {
                let Some(object) = value.as_object() else {
                    return Err("Record result must be an object".into());
                };
                if object.len() != fields.len() {
                    return Err("Record result has missing or extra fields".into());
                }
                for (field, field_type) in fields {
                    let Some(value) = object.get(field) else {
                        return Err("Record result has a missing field".into());
                    };
                    validate_value(value, field_type, types)
                        .map_err(|error| format!("Field '{field}': {error}"))?;
                }
                true
            }
            None => return Err("Referenced result type is not defined".into()),
        },
    };
    if valid {
        Ok(())
    } else {
        Err(format!(
            "Expected a {name} value within its supported limits"
        ))
    }
}

impl Flag {
    pub fn validate_result(&self, types: &BTreeMap<String, TypeDefinition>) -> Result<(), String> {
        self.validate()?;
        if let Self::Typed {
            result_type,
            selected,
            other,
            ..
        } = self
        {
            if result_type == "boolean" {
                return Err("Use the existing rollout modes for Boolean flags".into());
            }
            kind(result_type, types)?;
            validate_value(selected, result_type, types)
                .map_err(|error| format!("Selected result: {error}"))?;
            validate_value(other, result_type, types)
                .map_err(|error| format!("Other result: {error}"))?;
        }
        Ok(())
    }

    pub fn referenced_type(&self) -> Option<&str> {
        match self {
            Self::Typed { result_type, .. } if !BUILTINS.contains(&result_type.as_str()) => {
                Some(result_type)
            }
            _ => None,
        }
    }
}
