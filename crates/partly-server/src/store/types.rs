// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::{Store, audits::audit};
use crate::ApiError;
use partly_core::{Flag, TypeDefinition, validate_types};
use rusqlite::{Connection, params};
use serde_json::json;
use std::collections::BTreeMap;

impl Store {
    pub(super) fn read_types(
        db: &Connection,
        env: &str,
    ) -> Result<BTreeMap<String, TypeDefinition>, ApiError> {
        let mut query = db.prepare("SELECT name, definition FROM result_types WHERE environment = ?1 AND deleted = 0 ORDER BY name")?;
        let rows = query
            .query_map([env], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let types = rows
            .into_iter()
            .map(|(name, raw)| {
                serde_json::from_str(&raw)
                    .map(|definition| (name, definition))
                    .map_err(|_| ApiError::internal("Invalid stored result type"))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        validate_types(&types).map_err(ApiError::internal)?;
        Ok(types)
    }

    pub fn result_types(&self, env: &str) -> Result<BTreeMap<String, TypeDefinition>, ApiError> {
        self.environment(env)?;
        Self::read_types(self.db()?, env)
    }

    pub fn create_type(
        &mut self,
        actor: &str,
        env: &str,
        name: &str,
        definition: &TypeDefinition,
    ) -> Result<(), ApiError> {
        self.environment(env)?;
        let tx = self
            .connection
            .as_mut()
            .ok_or_else(|| ApiError::conflict("Complete setup first"))?
            .transaction()?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM result_types WHERE environment = ?1 AND name = ?2)",
            [env, name],
            |r| r.get(0),
        )?;
        if exists {
            return Err(ApiError::conflict(
                "Type name already used; deleted names remain reserved",
            ));
        }
        let mut types = Self::read_types(&tx, env)?;
        types.insert(name.into(), definition.clone());
        validate_types(&types).map_err(ApiError::bad)?;
        tx.execute(
            "INSERT INTO result_types (environment, name, definition) VALUES (?1, ?2, ?3)",
            params![
                env,
                name,
                serde_json::to_string(definition)
                    .map_err(|_| ApiError::internal("Cannot encode type"))?
            ],
        )?;
        audit(
            &tx,
            (actor, env),
            "created",
            "type",
            name,
            name,
            json!({"after": definition}),
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn delete_type(&mut self, actor: &str, env: &str, name: &str) -> Result<(), ApiError> {
        self.environment(env)?;
        let tx = self
            .connection
            .as_mut()
            .ok_or_else(|| ApiError::conflict("Complete setup first"))?
            .transaction()?;
        let types = Self::read_types(&tx, env)?;
        let definition = types
            .get(name)
            .ok_or_else(|| ApiError::not_found("Type not found"))?;
        if types.values().any(|d| matches!(d, TypeDefinition::Record { fields } if fields.values().any(|v| v == name)))
            || Self::read_policy(&tx, env, None)?.flags.values().any(|f| f.referenced_type() == Some(name)) {
            return Err(ApiError::conflict("Type is used by a flag or another type"));
        }
        tx.execute(
            "UPDATE result_types SET deleted = 1 WHERE environment = ?1 AND name = ?2",
            [env, name],
        )?;
        audit(
            &tx,
            (actor, env),
            "deleted",
            "type",
            name,
            name,
            json!({"before": definition}),
        )?;
        tx.commit()?;
        Ok(())
    }
}

pub(super) fn referenced_types(
    flags: &BTreeMap<String, Flag>,
    all: &BTreeMap<String, TypeDefinition>,
) -> Result<BTreeMap<String, TypeDefinition>, ApiError> {
    let mut types = BTreeMap::new();
    for name in flags.values().filter_map(Flag::referenced_type) {
        let definition = all
            .get(name)
            .ok_or_else(|| ApiError::internal("Flag references an unavailable type"))?;
        types.insert(name.into(), definition.clone());
        if let TypeDefinition::Record { fields } = definition {
            for name in fields.values() {
                if let Some(definition) = all.get(name) {
                    types.insert(name.clone(), definition.clone());
                }
            }
        }
    }
    Ok(types)
}
