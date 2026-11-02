// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use super::{Resolution, TypeDefinition, validation::kind};
use crate::{Context, EvaluationError, Flag, Policy};
use serde_json::{Number, Value};

impl Policy {
    /// Resolve locally. `named` requires an exact expected type name; other kinds use base types.
    pub fn resolve(
        &self,
        key: &str,
        context: &Context,
        expected: &str,
        expected_name: Option<&str>,
    ) -> Result<Resolution, EvaluationError> {
        let flag = self
            .flags
            .get(key)
            .ok_or_else(|| EvaluationError::FlagNotFound(key.into()))?;
        let (actual, name) = match flag {
            Flag::Typed { result_type, .. } => (
                kind(result_type, &self.types).expect("Accepted policies have validated types"),
                flag.referenced_type(),
            ),
            _ => ("boolean", None),
        };
        if expected == "named" {
            if name.is_none() || name != expected_name {
                return Err(EvaluationError::NamedType {
                    flag_key: key.into(),
                    expected_type: expected_name.unwrap_or("").into(),
                    actual_type: name.unwrap_or(actual).into(),
                });
            }
        } else if expected != actual
            && !(expected == "number" && matches!(actual, "integer" | "float"))
        {
            return Err(EvaluationError::ResultType {
                flag_key: key.into(),
                expected_type: expected.into(),
                actual_type: actual.into(),
            });
        }
        let rollout = match flag {
            Flag::Typed { rollout, .. } => rollout.as_ref(),
            _ => flag,
        };
        let selected = rollout.select(key, context)?;
        let value = match flag {
            Flag::Typed {
                selected: yes,
                other,
                result_type,
                ..
            } => normalize(if selected { yes } else { other }, result_type, &self.types),
            _ => Value::Bool(selected),
        };
        Ok(Resolution {
            value,
            kind: actual.into(),
            named_type: name.map(str::to_owned),
            variant: match (actual, selected) {
                ("boolean", true) => "enabled",
                ("boolean", false) => "disabled",
                (_, true) => "selected",
                (_, false) => "other",
            }
            .into(),
        })
    }
}

// Preserve declared float fields too; returned records never share mutable cache storage.
fn normalize(
    value: &Value,
    name: &str,
    types: &std::collections::BTreeMap<String, TypeDefinition>,
) -> Value {
    if name == "float" {
        return Value::Number(Number::from_f64(value.as_f64().unwrap()).unwrap());
    }
    if let Some(TypeDefinition::Record { fields }) = types.get(name) {
        return Value::Object(
            fields
                .iter()
                .map(|(key, name)| (key.clone(), normalize(&value[key], name, types)))
                .collect(),
        );
    }
    value.clone()
}
