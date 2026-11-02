// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use crate::{Context, Decision, EvaluationError, Flag, MatchAction, Policy, value_type};
use serde_json::Value;
use sha2::{Digest, Sha256};

impl Policy {
    pub fn evaluate(&self, key: &str, context: &Context) -> Result<Decision, EvaluationError> {
        let flag = self
            .flags
            .get(key)
            .ok_or_else(|| EvaluationError::FlagNotFound(key.into()))?;
        if let Flag::Typed { .. } = flag {
            self.resolve(key, context, "boolean", None)?;
            unreachable!("Typed flags cannot return Boolean decisions");
        }
        Ok(if flag.select(key, context)? {
            Decision::Enabled
        } else {
            Decision::Disabled
        })
    }
}

impl Flag {
    pub(crate) fn select(&self, key: &str, context: &Context) -> Result<bool, EvaluationError> {
        let flag = self;
        // Validate every required field before matching: an early match must not hide an error.
        match flag {
            Flag::Percentage { .. } => {
                let id = required(context, key, "id", None, "string")?
                    .as_str()
                    .unwrap();
                if id.trim().is_empty() || id.len() > 256 {
                    return Err(EvaluationError::InvalidContext {
                        flag_key: key.into(),
                        k: "id".into(),
                        reason: "id must contain 1–256 UTF-8 bytes and cannot be blank",
                    });
                }
            }
            Flag::Custom { groups, .. } => {
                for condition in groups.iter().flatten() {
                    required(
                        context,
                        key,
                        &condition.k,
                        Some(condition.op.name()),
                        condition.op.expected_type(),
                    )?;
                }
            }
            _ => {}
        }
        for (k, v) in context {
            if !matches!(v, Value::String(_) | Value::Number(_) | Value::Bool(_)) {
                return Err(EvaluationError::ContextType {
                    flag_key: key.into(),
                    k: k.clone(),
                    op: None,
                    expected_type: "string, number, or boolean",
                    actual_type: value_type(v),
                });
            }
        }
        let enabled = match flag {
            Flag::Enabled {} => true,
            Flag::Disabled {} => false,
            Flag::Percentage { percentage } => {
                bucket(key, context["id"].as_str().unwrap()) < u32::from(*percentage) * 100
            }
            Flag::Typed { .. } => unreachable!("Typed rollouts are validated before evaluation"),
            Flag::Custom { action, groups } => {
                let matched = groups.iter().any(|group| {
                    group
                        .iter()
                        .all(|condition| condition.matches(&context[&condition.k]))
                });
                matched == matches!(action, MatchAction::Enable)
            }
        };
        Ok(enabled)
    }
}

fn required<'a>(
    context: &'a Context,
    flag_key: &str,
    k: &str,
    op: Option<&'static str>,
    expected_type: &'static str,
) -> Result<&'a Value, EvaluationError> {
    let value = context
        .get(k)
        .ok_or_else(|| EvaluationError::InvalidContext {
            flag_key: flag_key.into(),
            k: k.into(),
            reason: "required key is missing",
        })?;
    let actual_type = value_type(value);
    if actual_type != expected_type {
        return Err(EvaluationError::ContextType {
            flag_key: flag_key.into(),
            k: k.into(),
            op,
            expected_type,
            actual_type,
        });
    }
    Ok(value)
}
// Versioned, length-prefixed input avoids ambiguous concatenation and process-specific hashes.
pub fn bucket(key: &str, user_id: &str) -> u32 {
    let mut hash = Sha256::new();
    hash.update(b"partly:v1:");
    for value in [key, user_id] {
        hash.update((value.len() as u32).to_be_bytes());
        hash.update(value.as_bytes());
    }
    let digest = hash.finalize();
    u32::from_be_bytes(digest[..4].try_into().unwrap()) % 10_000
}
