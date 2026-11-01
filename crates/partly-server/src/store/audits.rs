// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::Store;
use super::models::AuditEntry;
use crate::ApiError;
use rusqlite::{Connection, params};
use serde_json::Value;

pub(super) fn audit(
    db: &Connection,
    scope: (&str, &str),
    action: &str,
    kind: &str,
    id: &str,
    name: &str,
    details: Value,
) -> Result<(), ApiError> {
    db.execute(
        "INSERT INTO audits (environment, actor, action, item_type, item_id, item_name, \
            application_id, details) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            scope.1,
            scope.0,
            action,
            kind,
            id,
            name,
            details.get("application_id").and_then(Value::as_str),
            details.to_string()
        ],
    )?;
    Ok(())
}

impl Store {
    pub fn audits(
        &self,
        env: &str,
        before: Option<i64>,
        action: Option<&str>,
        app: Option<&str>,
    ) -> Result<Vec<AuditEntry>, ApiError> {
        self.environment(env)?;
        let mut query = self.db()?.prepare(
            "SELECT id, actor, action, item_type, item_id, item_name, details, created_at FROM \
            audits WHERE environment = ?1 AND (?2 IS NULL OR id < ?2) AND (?3 IS NULL OR action \
            = ?3) AND (?4 IS NULL OR application_id = ?4 OR EXISTS (SELECT 1 FROM \
            json_each(details, '$.applications') WHERE json_extract(value, '$.id') = ?4)) ORDER \
            BY id DESC LIMIT 100",
        )?;
        let rows = query
            .query_map(params![env, before, action, app], |r| {
                let raw: String = r.get(6)?;
                let details: Value = serde_json::from_str(&raw).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        6,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                Ok(AuditEntry {
                    id: r.get(0)?,
                    actor: r.get(1)?,
                    action: r.get(2)?,
                    item_type: r.get(3)?,
                    item_id: r.get(4)?,
                    item_name: r.get(5)?,
                    details,
                    created_at: r.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}
