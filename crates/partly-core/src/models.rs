// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use crate::TypeDefinition;
#[cfg(feature = "openapi")]
use crate::openapi;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use thiserror::Error;

pub type Context = BTreeMap<String, Value>;
pub const MAX_POLICY_BYTES: usize = 1024 * 1024;

// Utoipa needs property limits on variants to mirror Serde's deny_unknown_fields.
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Flag {
    #[cfg_attr(feature = "openapi", schema(max_properties = 5))]
    Typed {
        result_type: String,
        selected: Value,
        other: Value,
        #[cfg_attr(feature = "openapi", schema(schema_with = openapi::rollout))]
        rollout: Box<Flag>,
    },
    #[cfg_attr(feature = "openapi", schema(max_properties = 1))]
    Enabled {},
    #[cfg_attr(feature = "openapi", schema(max_properties = 1))]
    Disabled {},
    #[cfg_attr(feature = "openapi", schema(max_properties = 2))]
    Percentage {
        #[cfg_attr(feature = "openapi", schema(minimum = 0, maximum = 100))]
        percentage: u8,
    },
    #[cfg_attr(feature = "openapi", schema(max_properties = 3))]
    Custom {
        action: MatchAction,
        #[cfg_attr(feature = "openapi", schema(schema_with = openapi::groups))]
        groups: Vec<Vec<Condition>>,
    },
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchAction {
    Enable,
    Disable,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operator {
    Le,
    Lt,
    Ge,
    Gt,
    Eq,
    Prefix,
    Suffix,
    Contains,
    Matches,
    Regex,
    Is,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "RawCondition")]
pub struct Condition {
    pub(crate) k: String,
    pub(crate) op: Operator,
    pub(crate) v: Value,
    #[serde(skip)]
    pub(crate) regex: Option<Regex>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawCondition {
    pub(crate) k: String,
    pub(crate) op: Operator,
    pub(crate) v: Value,
}

#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    #[cfg_attr(feature = "openapi", schema(minimum = 1, maximum = 2))]
    pub schema_version: u32,
    pub project: String,
    pub environment: String,
    pub application_id: String,
    pub revision: u64,
    pub flags: BTreeMap<String, Flag>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub types: BTreeMap<String, TypeDefinition>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Enabled,
    Disabled,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EvaluationError {
    #[error(
        "Result type mismatch for flag '{flag_key}': expected {expected_type}, got {actual_type}"
    )]
    ResultType {
        flag_key: String,
        expected_type: String,
        actual_type: String,
    },
    #[error(
        "Named type mismatch for flag '{flag_key}': expected {expected_type}, got {actual_type}"
    )]
    NamedType {
        flag_key: String,
        expected_type: String,
        actual_type: String,
    },
    #[error("Flag '{0}' is not defined in the loaded policy")]
    FlagNotFound(String),
    #[error("Invalid context for flag '{flag_key}', key '{k}': {reason}")]
    InvalidContext {
        flag_key: String,
        k: String,
        reason: &'static str,
    },
    #[error(
        "Context type mismatch for flag '{flag_key}', key '{k}': expected {expected_type}, got {actual_type}"
    )]
    ContextType {
        flag_key: String,
        k: String,
        op: Option<&'static str>,
        expected_type: &'static str,
        actual_type: &'static str,
    },
}
