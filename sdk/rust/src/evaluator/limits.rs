// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use super::{Condition, Flag, Operator, models::RawCondition};
use serde::{Deserialize, Deserializer, de::Error};
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) const MAX_POLICY_REGEXES: usize = 64;
pub(crate) const MAX_REGEX_BYTES: usize = 1024;
pub(crate) const MAX_COMPILED_REGEX_BYTES: usize = 256 * 1024;

pub(super) fn check_regex_budget(regexes: usize) -> Result<(), String> {
    if regexes > MAX_POLICY_REGEXES {
        return Err("Policy exceeds 64 regex rules".into());
    }
    Ok(())
}

pub(super) fn deserialize_groups<'de, D>(deserializer: D) -> Result<Vec<Vec<Condition>>, D::Error>
where
    D: Deserializer<'de>,
{
    let groups = Vec::<Vec<RawCondition>>::deserialize(deserializer)?;
    let regexes = groups
        .iter()
        .flatten()
        .filter(|c| matches!(c.op, Operator::Regex))
        .count();
    // Check the whole input before compiling even its first regular expression.
    check_regex_budget(regexes).map_err(D::Error::custom)?;
    groups
        .into_iter()
        .map(|group| {
            group
                .into_iter()
                .map(|raw| Condition::try_from(raw).map_err(D::Error::custom))
                .collect()
        })
        .collect()
}

pub(super) fn deserialize_flags<'de, D>(deserializer: D) -> Result<BTreeMap<String, Flag>, D::Error>
where
    D: Deserializer<'de>,
{
    let flags = BTreeMap::<String, Value>::deserialize(deserializer)?;
    let mut regexes = 0;
    for flag in flags.values() {
        let rollout = if flag["mode"] == "typed" {
            &flag["rollout"]
        } else {
            flag
        };
        if rollout["mode"] == "typed" {
            return Err(D::Error::custom("Typed rollouts cannot be nested"));
        }
        if let Some(groups) = rollout.get("groups").and_then(Value::as_array) {
            for condition in groups.iter().filter_map(Value::as_array).flatten() {
                regexes += usize::from(condition["op"] == "regex");
            }
        }
    }
    check_regex_budget(regexes).map_err(D::Error::custom)?;
    flags
        .into_iter()
        .map(|(key, raw)| {
            serde_json::from_value(raw)
                .map(|flag| (key, flag))
                .map_err(D::Error::custom)
        })
        .collect()
}

impl Flag {
    pub(super) fn regex_count(&self) -> usize {
        match self {
            Self::Typed { rollout, .. } => rollout.regex_count(),
            Self::Custom { groups, .. } => groups
                .iter()
                .flatten()
                .filter(|c| matches!(c.op, Operator::Regex))
                .count(),
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests;
