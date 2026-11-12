// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::models::AppReference;
use super::{Store, audits::audit};
use crate::ApiError;
use partly::evaluator::{Flag, MAX_POLICY_BYTES, Policy};
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) fn policy_format(flags: &BTreeMap<String, Flag>) -> u32 {
    if flags
        .values()
        .any(|flag| matches!(flag, Flag::Typed { .. }))
    {
        2
    } else {
        1
    }
}

pub(super) fn check_policy_size(policy: &Policy) -> Result<(), ApiError> {
    policy.validate().map_err(ApiError::bad)?;
    let size = serde_json::to_vec(policy)
        .map_err(|_| ApiError::internal("Cannot encode policy"))?
        .len();
    // Reserve enough metadata for any application ID and revision in this environment.
    let scope_reserve = 64_usize.saturating_sub(policy.application_id.len()) + 20
        - policy.revision.to_string().len();
    if size + scope_reserve > MAX_POLICY_BYTES {
        return Err(ApiError::bad(
            "Policy exceeds the 1 MiB POC limit; remove flags or simplify rules",
        ));
    }
    Ok(())
}

impl Store {
    pub fn environment(&self, env: &str) -> Result<(), ApiError> {
        if !matches!(env, "development" | "production") {
            return Err(ApiError::not_found("Unknown environment"));
        }
        Ok(())
    }

    pub fn policy(&self, env: &str) -> Result<Policy, ApiError> {
        self.environment(env)?;
        let revision: i64 = self.db()?.query_row(
            "SELECT revision FROM environments WHERE name = ?1",
            [env],
            |row| row.get(0),
        )?;
        let revision =
            u64::try_from(revision).map_err(|_| ApiError::internal("Invalid policy revision"))?;
        if let Some(policy) = self
            .policies
            .borrow()
            .get(env)
            .filter(|p| p.revision == revision)
        {
            return Ok(policy.clone());
        }
        let policy = Self::read_policy(self.db()?, env)?;
        check_policy_size(&policy)?;
        self.policies
            .borrow_mut()
            .insert(env.into(), policy.clone());
        Ok(policy)
    }

    fn read_policy(db: &Connection, env: &str) -> Result<Policy, ApiError> {
        let (project, revision): (String, i64) = db.query_row(
            "SELECT project, revision FROM environments WHERE name = ?1",
            [env],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let revision =
            u64::try_from(revision).map_err(|_| ApiError::internal("Invalid policy revision"))?;
        let mut statement =
            db.prepare("SELECT key, config FROM flags WHERE environment = ?1 ORDER BY key")?;
        let rows = statement
            .query_map([env], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if rows.iter().map(|(_, config)| config.len()).sum::<usize>() > MAX_POLICY_BYTES {
            return Err(ApiError::internal(
                "Stored policy exceeds the 1 MiB POC limit",
            ));
        }
        let flags = rows
            .into_iter()
            .map(|(key, config)| {
                let flag: Value = serde_json::from_str(&config)
                    .map_err(|_| ApiError::internal("Invalid stored flag configuration"))?;
                partly::evaluator::validate_json_number_limits(config.as_bytes())
                    .map_err(|_| ApiError::internal("Invalid stored flag configuration"))?;
                Ok((key, flag))
            })
            .collect::<Result<BTreeMap<_, _>, ApiError>>()?;
        // Use the same pre-compilation budget as SDK downloads, including on restart.
        let mut policy: Policy = serde_json::from_value(json!({
            "schema_version": 2, "project": project, "environment": env,
            "application_id": "environment", "revision": revision,
            "flags": flags, "types": Self::read_types(db, env)?,
        }))
        .map_err(|_| ApiError::internal("Invalid stored policy or rule limits exceeded"))?;
        policy.schema_version = policy_format(&policy.flags);
        policy.types = super::types::referenced_types(&policy.flags, &policy.types)?;
        Ok(policy)
    }

    pub fn save_flag(
        &mut self,
        actor: &str,
        env: &str,
        key: &str,
        flag: &Flag,
        create: bool,
    ) -> Result<(), ApiError> {
        self.environment(env)?;
        if !partly::evaluator::valid_key(key) {
            return Err(ApiError::bad("Invalid flag key"));
        }
        flag.validate_result(&self.result_types(env)?)
            .map_err(ApiError::bad)?;
        let config =
            serde_json::to_string(flag).map_err(|_| ApiError::internal("Cannot encode flag"))?;
        let mut policy = self.policy(env)?;
        let previous = policy.flags.get(key).cloned();
        policy.flags.insert(key.into(), flag.clone());
        policy.schema_version = policy_format(&policy.flags);
        policy.types = super::types::referenced_types(&policy.flags, &self.result_types(env)?)?;
        let transaction = self
            .connection
            .as_mut()
            .ok_or_else(|| ApiError::conflict("Complete setup first"))?
            .transaction()?;
        let before: Option<String> = transaction
            .query_row(
                "SELECT config FROM flags WHERE environment = ?1 AND key = ?2",
                [env, key],
                |r| r.get(0),
            )
            .optional()?;
        let exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM flags WHERE environment = ?1 AND key = ?2)",
            [env, key],
            |r| r.get(0),
        )?;
        if create && exists {
            return Err(ApiError::conflict(
                "A flag with this key already exists in this environment",
            ));
        }
        if !create && !exists {
            return Err(ApiError::not_found("Flag not found"));
        }
        if let Some(previous) = &previous {
            let result_type = |f: &Flag| match f {
                Flag::Typed { result_type, .. } => result_type.clone(),
                _ => "boolean".into(),
            };
            if result_type(previous) != result_type(flag) {
                return Err(ApiError::bad(
                    "A flag's result type cannot change; create a new flag",
                ));
            }
        }
        let changed = transaction.execute(
            "INSERT INTO flags (environment, key, config) VALUES (?1, ?2, ?3) ON \
            CONFLICT(environment, key) DO UPDATE SET config = excluded.config WHERE config != \
            excluded.config",
            params![env, key, config],
        )?;
        if changed > 0 {
            transaction.execute(
                "UPDATE environments SET revision = revision + 1 WHERE name = ?1",
                [env],
            )?;
            transaction.execute("UPDATE applications SET revision = revision + 1 WHERE id IN (SELECT application_id \
            FROM application_flags WHERE environment = ?1 AND flag_key = ?2)", [env, key])?;
            policy.revision += 1;
            check_policy_size(&policy)?;
            let before: Option<Value> = before
                .map(|v| serde_json::from_str(&v))
                .transpose()
                .map_err(|_| ApiError::internal("Invalid stored flag"))?;
            audit(
                &transaction,
                (actor, env),
                if create { "created" } else { "updated" },
                "flag",
                key,
                key,
                json!({"before": before, "after": flag, "applications": Self::flag_apps(&transaction, env, key)?}),
            )?;
        }
        transaction.commit()?;
        // Publish only committed state; reads reuse the compiled rules across applications.
        if changed > 0 {
            self.policies.borrow_mut().insert(env.into(), policy);
        }
        Ok(())
    }

    pub fn delete_flag(&mut self, actor: &str, env: &str, key: &str) -> Result<(), ApiError> {
        self.environment(env)?;
        let mut policy = self.policy(env)?;
        policy.flags.remove(key);
        policy.schema_version = policy_format(&policy.flags);
        policy.types = super::types::referenced_types(&policy.flags, &policy.types)?;
        policy.revision += 1;
        let transaction = self
            .connection
            .as_mut()
            .ok_or_else(|| ApiError::conflict("Complete setup first"))?
            .transaction()?;
        let before: Option<String> = transaction
            .query_row(
                "SELECT config FROM flags WHERE environment = ?1 AND key = ?2",
                [env, key],
                |r| r.get(0),
            )
            .optional()?;
        let applications = Self::flag_apps(&transaction, env, key)?;
        transaction.execute(
            "UPDATE applications SET revision = revision + 1 WHERE id IN (SELECT application_id \
            FROM application_flags WHERE environment = ?1 AND flag_key = ?2)",
            [env, key],
        )?;
        if transaction.execute(
            "DELETE FROM flags WHERE environment = ?1 AND key = ?2",
            [env, key],
        )? == 0
        {
            return Err(ApiError::not_found("Flag not found"));
        }
        transaction.execute(
            "UPDATE environments SET revision = revision + 1 WHERE name = ?1",
            [env],
        )?;
        let before: Option<Value> = before
            .map(|v| serde_json::from_str(&v))
            .transpose()
            .map_err(|_| ApiError::internal("Invalid stored flag"))?;
        audit(
            &transaction,
            (actor, env),
            "deleted",
            "flag",
            key,
            key,
            json!({"before": before, "after": null, "applications": applications}),
        )?;
        transaction.commit()?;
        self.policies.borrow_mut().insert(env.into(), policy);
        Ok(())
    }

    fn flag_apps(db: &Connection, env: &str, key: &str) -> Result<Vec<AppReference>, ApiError> {
        let mut query = db.prepare(
            "SELECT a.id, a.name FROM applications a JOIN application_flags f ON a.id = \
            f.application_id WHERE f.environment = ?1 AND f.flag_key = ?2 ORDER BY a.name, a.id",
        )?;
        let rows = query
            .query_map([env, key], |r| {
                Ok(AppReference {
                    id: r.get(0)?,
                    name: r.get(1)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn flag_impact(&self, env: &str, key: &str) -> Result<Vec<AppReference>, ApiError> {
        self.environment(env)?;
        if !self.policy(env)?.flags.contains_key(key) {
            return Err(ApiError::not_found("Flag not found"));
        }
        Self::flag_apps(self.db()?, env, key)
    }
}
