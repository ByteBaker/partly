// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use crate::{Flag, MAX_POLICY_BYTES, Policy, validate_json_number_limits};

impl Flag {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Percentage { percentage } if *percentage > 100 => {
                Err("Percentage must be 0–100".into())
            }
            Self::Custom { groups, .. }
                if groups.is_empty() || groups.iter().any(Vec::is_empty) =>
            {
                Err("Custom rules need at least one group and one condition in each group".into())
            }
            // Conditions are immutable and validated, including compiled regex, on deserialization.
            _ => Ok(()),
        }
    }
}

pub fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"_-".contains(&c))
}

impl Policy {
    pub fn from_json(json: &[u8]) -> Result<Self, String> {
        if json.len() > MAX_POLICY_BYTES {
            return Err("Policy exceeds the 1 MiB POC limit".into());
        }
        let policy: Self = serde_json::from_slice(json).map_err(|e| e.to_string())?;
        policy.validate()?;
        validate_json_number_limits(json)?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("Unsupported policy schema_version".into());
        }
        if !valid_key(&self.project)
            || !valid_key(&self.environment)
            || !valid_key(&self.application_id)
        {
            return Err("Invalid policy project, environment, or application".into());
        }
        for (key, flag) in &self.flags {
            if !valid_key(key) {
                return Err("Invalid flag key".into());
            }
            flag.validate()?;
        }
        Ok(())
    }
}
