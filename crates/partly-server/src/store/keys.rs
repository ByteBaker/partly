// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::{Store, audits::audit, models::SdkKey, secret};
use crate::ApiError;
use partly_core::Policy;
use rand::{
    distributions::{Alphanumeric, DistString},
    rngs::OsRng,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;
use sha2::{Digest, Sha256};

pub(super) fn key_name(name: &str) -> Result<&str, ApiError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(ApiError::bad(
            "Use a name of 1–64 characters without control characters",
        ));
    }
    Ok(name)
}

impl Store {
    pub fn key_environment(&self, id: &str) -> Result<String, ApiError> {
        self.db()?
            .query_row(
                "SELECT a.environment FROM sdk_keys k JOIN applications a \
                 ON a.id = k.application_id WHERE k.id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| ApiError::not_found("Key not found"))
    }

    pub(super) fn read_keys(db: &Connection, application: &str) -> Result<Vec<SdkKey>, ApiError> {
        let mut query = db.prepare(
            "SELECT id, name, secret, masked_secret, created_at, revoked_at, (SELECT \
            MAX(last_used_at) FROM flag_usage u WHERE u.api_key_id = sdk_keys.id) FROM sdk_keys \
            WHERE application_id = ?1 ORDER BY rowid",
        )?;
        let rows = query
            .query_map([application], |r| {
                Ok(SdkKey {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    secret: r.get(2)?,
                    masked_secret: r.get(3)?,
                    created_at: r.get(4)?,
                    revoked_at: r.get(5)?,
                    last_used_at: r.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub(super) fn insert_key(
        db: &Connection,
        actor: &str,
        env: &str,
        app: &str,
        name: &str,
    ) -> Result<SdkKey, ApiError> {
        let name = key_name(name)?;
        let id = secret()[..16].to_owned();
        let key = format!("pk_{}", Alphanumeric.sample_string(&mut OsRng, 24));
        let masked = format!("{}••••••••••••{}", &key[..7], &key[key.len() - 4..]);
        let hash = format!("{:x}", Sha256::digest(key.as_bytes()));
        // shortcut: SDK keys are plaintext in the local POC, add protected storage before production use.
        db.execute(
            "INSERT INTO sdk_keys (id, application_id, secret, secret_hash, masked_secret, name) \
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, app, key, hash, masked, name],
        )?;
        audit(
            db,
            (actor, env),
            "created",
            "api_key",
            &id,
            name,
            json!({"application_id": app, "masked_secret": masked}),
        )?;
        Self::read_keys(db, app)?
            .into_iter()
            .find(|key| key.id == id)
            .ok_or_else(|| ApiError::internal("Cannot read new API Key"))
    }

    pub fn create_key(
        &mut self,
        actor: &str,
        env: &str,
        app: &str,
        name: &str,
    ) -> Result<SdkKey, ApiError> {
        self.environment(env)?;
        let tx = self
            .connection
            .as_mut()
            .ok_or_else(|| ApiError::conflict("Complete setup first"))?
            .transaction()?;
        Self::read_application(&tx, env, app)?;
        let key = Self::insert_key(&tx, actor, env, app, name)?;
        tx.commit()?;
        Ok(key)
    }

    pub fn change_key(
        &mut self,
        actor: &str,
        id: &str,
        name: Option<&str>,
    ) -> Result<(), ApiError> {
        let name = name.map(key_name).transpose()?;
        let tx = self
            .connection
            .as_mut()
            .ok_or_else(|| ApiError::conflict("Complete setup first"))?
            .transaction()?;
        let (app, env, old_name, revoked): (String, String, String, Option<String>) = tx.query_row("SELECT a.id, a.environment, k.name, k.revoked_at FROM sdk_keys k JOIN applications a \
            ON a.id = k.application_id WHERE k.id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).optional()?.ok_or_else(|| ApiError::not_found("Key not found"))?;
        if revoked.is_some() {
            return Err(ApiError::conflict("Key is already revoked"));
        }
        if let Some(name) = name {
            if name != old_name {
                tx.execute("UPDATE sdk_keys SET name = ?1 WHERE id = ?2", [name, id])?;
                audit(
                    &tx,
                    (actor, &env),
                    "renamed",
                    "api_key",
                    id,
                    name,
                    json!({"application_id": app, "before": {"name": old_name}, "after": {"name": name}}),
                )?;
            }
        } else {
            tx.execute("UPDATE sdk_keys SET secret = NULL, revoked_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1", [id])?;
            audit(
                &tx,
                (actor, &env),
                "revoked",
                "api_key",
                id,
                &old_name,
                json!({"application_id": app}),
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn app_policy(&self, key: &str, env: &str, app: &str) -> Result<Policy, ApiError> {
        let (_, revision) = Self::authorize_key(self.db()?, key, env, app)?;
        self.environment(env)?;
        let mut policy = Self::read_policy(self.db()?, env, Some(app))?;
        policy.application_id = app.into();
        policy.revision = u64::try_from(revision)
            .map_err(|_| ApiError::internal("Invalid application revision"))?;
        Ok(policy)
    }

    pub(super) fn authorize_key(
        db: &Connection,
        key: &str,
        env: &str,
        app: &str,
    ) -> Result<(String, i64), ApiError> {
        let hash = format!("{:x}", Sha256::digest(key.as_bytes()));
        let (scope_env, scope_app, revoked, revision, id): (
            String,
            String,
            Option<String>,
            i64,
            String,
        ) = db
            .query_row(
                "SELECT a.environment, a.id, k.revoked_at, a.revision, k.id FROM sdk_keys k JOIN \
            applications a ON a.id = k.application_id WHERE k.secret_hash = ?1",
                [hash],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .optional()?
            .ok_or_else(ApiError::unauthorized)?;
        if revoked.is_some() {
            return Err(ApiError(
                axum::http::StatusCode::UNAUTHORIZED,
                "key_revoked".into(),
            ));
        }
        if env != scope_env || app != scope_app {
            return Err(ApiError::unauthorized());
        }
        Ok((id, revision))
    }
}
