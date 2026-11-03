// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::store;
pub(crate) use crate::store::{AppReference, AuditEntry};
use partly::evaluator::Flag;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(utoipa::ToSchema, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Credentials {
    pub(crate) username: String,
    #[schema(write_only = true)]
    pub(crate) password: String,
}

#[derive(Deserialize, Serialize)]
pub(crate) struct SavedFlag {
    pub(crate) key: String,
    #[serde(flatten)]
    pub(crate) flag: Flag,
}

#[derive(utoipa::ToSchema, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApplicationInput {
    pub(crate) name: String,
    pub(crate) flags: Vec<String>,
}

#[derive(utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AuditQuery {
    pub(crate) before: Option<i64>,
    pub(crate) action: Option<String>,
    pub(crate) application_id: Option<String>,
}

#[derive(utoipa::ToSchema, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct KeyName {
    pub(crate) name: String,
}

#[derive(Serialize, ToSchema)]
pub struct ErrorBody {
    pub error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<RevocationCode>,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum RevocationCode {
    KeyRevoked,
}

#[derive(Serialize, ToSchema)]
pub struct Status {
    pub setup_required: bool,
    pub authenticated: bool,
}

#[derive(Serialize, ToSchema)]
pub struct LoginResult {
    pub ok: bool,
}

#[derive(Serialize, ToSchema)]
pub struct Environments {
    pub project: &'static str,
    pub environments: Vec<&'static str>,
    pub user: store::User,
}

#[derive(Serialize, ToSchema)]
pub struct Users {
    pub users: Vec<store::User>,
}

#[derive(Serialize, ToSchema)]
pub struct Applications {
    pub applications: Vec<store::Application>,
}

#[derive(Serialize, ToSchema)]
pub struct Usage {
    pub usage: Vec<store::FlagUsage>,
}

#[derive(Serialize, ToSchema)]
pub struct Impact {
    pub applications: Vec<AppReference>,
    /// Supply this token unchanged as X-Partly-Impact when saving or deleting.
    pub token: String,
}

#[derive(Serialize, ToSchema)]
pub struct Audits {
    #[schema(max_items = 100)]
    pub audits: Vec<AuditEntry>,
}

#[derive(Deserialize, Serialize, ToSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct NamedTypeInput {
    pub name: String,
    pub definition: partly::evaluator::TypeDefinition,
}
