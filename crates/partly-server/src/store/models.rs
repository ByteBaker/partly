// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(
    utoipa::ToSchema, Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord,
)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Viewer,
    Editor,
    Manager,
}

#[derive(utoipa::ToSchema, Clone, Serialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub is_admin: bool,
    pub enabled: bool,
    pub permissions: BTreeMap<String, Role>,
}

#[derive(utoipa::ToSchema, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserInput {
    pub username: String,
    pub is_admin: bool,
    pub enabled: bool,
    pub permissions: BTreeMap<String, Role>,
    /// Required when creating an account; omit or use null to keep it on update.
    #[schema(write_only = true)]
    pub password: Option<String>,
}

#[derive(utoipa::ToSchema, Serialize)]
pub struct SdkKey {
    pub(super) id: String,
    pub(super) name: String,
    /// Null for revoked keys and for viewers/editors. Never returned after revocation.
    #[schema(required = true)]
    pub(super) secret: Option<String>,
    pub(super) masked_secret: String,
    pub(super) created_at: String,
    #[schema(required = true)]
    pub(super) revoked_at: Option<String>,
    #[schema(required = true)]
    pub(super) last_used_at: Option<String>,
}

#[derive(utoipa::ToSchema, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageInput {
    #[schema(max_items = 128)]
    pub flags: Vec<String>,
}

#[derive(utoipa::ToSchema, Serialize)]
pub struct FlagUsage {
    pub(super) api_key_id: String,
    pub(super) key_name: String,
    pub(super) application_id: String,
    pub(super) application_name: String,
    pub(super) flag_key: String,
    pub(super) last_used_at: String,
}

#[derive(utoipa::ToSchema, Serialize)]
pub struct Application {
    pub(super) id: String,
    pub(super) name: String,
    pub(super) flags: Vec<String>,
    pub(super) keys: Vec<SdkKey>,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct AppReference {
    pub id: String,
    pub name: String,
}

#[derive(Serialize, utoipa::ToSchema)]
pub struct AuditEntry {
    pub id: i64,
    pub actor: String,
    pub action: String,
    pub item_type: String,
    pub item_id: String,
    pub item_name: String,
    /// Event-specific states: before/after, affected applications, or key/account metadata.
    /// Creation has a null before; deletion has a null after. Never includes full credentials.
    #[schema(value_type = Object)]
    pub details: serde_json::Value,
    /// Server UTC timestamp in SQLite's YYYY-MM-DD HH:MM:SS format.
    pub created_at: String,
}
