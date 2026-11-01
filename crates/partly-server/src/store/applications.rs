// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::{Store, audits::audit, keys::key_name, models::Application, secret};
use crate::ApiError;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;

impl Application {
    pub fn hide_secrets(&mut self) {
        for key in &mut self.keys {
            key.secret = None;
        }
    }
}

impl Store {
    pub(super) fn read_application(
        db: &Connection,
        env: &str,
        id: &str,
    ) -> Result<Application, ApiError> {
        let name = db
            .query_row(
                "SELECT name FROM applications WHERE environment = ?1 AND id = ?2",
                [env, id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| ApiError::not_found("Application not found"))?;
        let mut query = db.prepare(
            "SELECT flag_key FROM application_flags WHERE application_id = ?1 ORDER BY flag_key",
        )?;
        let flags = query
            .query_map([id], |r| r.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        let keys = Self::read_keys(db, id)?;
        Ok(Application {
            id: id.into(),
            name,
            flags,
            keys,
        })
    }

    pub fn applications(&self, env: &str) -> Result<Vec<Application>, ApiError> {
        self.environment(env)?;
        let db = self.db()?;
        let mut query =
            db.prepare("SELECT id FROM applications WHERE environment = ?1 ORDER BY name, id")?;
        let ids = query
            .query_map([env], |r| r.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        ids.iter()
            .map(|id| Self::read_application(db, env, id))
            .collect()
    }

    pub fn save_application(
        &mut self,
        actor: &str,
        env: &str,
        id: Option<&str>,
        name: &str,
        flags: &[String],
    ) -> Result<Application, ApiError> {
        self.environment(env)?;
        let name = key_name(name)?;
        let created = id.is_none();
        let id = id
            .map(str::to_owned)
            .unwrap_or_else(|| format!("app_{}", &secret()[..20]));
        let tx = self
            .connection
            .as_mut()
            .ok_or_else(|| ApiError::conflict("Complete setup first"))?
            .transaction()?;
        let before = if created {
            None
        } else {
            Some(Self::read_application(&tx, env, &id)?)
        };
        let duplicate: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM applications WHERE environment = ?1 AND name = ?2 AND id != ?3)", params![env, name, id], |r| r.get(0))?;
        if duplicate {
            return Err(ApiError::conflict(
                "An application with this name already exists in this environment",
            ));
        }
        if created {
            tx.execute(
                "INSERT INTO applications (id, environment, name) VALUES (?1, ?2, ?3)",
                params![id, env, name],
            )?;
        } else {
            tx.execute(
                "UPDATE applications SET name = ?1 WHERE id = ?2",
                params![name, id],
            )?;
        }
        tx.execute(
            "DELETE FROM application_flags WHERE application_id = ?1",
            [&id],
        )?;
        let mut selected = std::collections::BTreeSet::new();
        for flag in flags {
            if !selected.insert(flag) {
                return Err(ApiError::bad("Select each flag only once"));
            }
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM flags WHERE environment = ?1 AND key = ?2)",
                [env, flag],
                |r| r.get(0),
            )?;
            if !exists {
                return Err(ApiError::bad(
                    "Selected flag does not exist in this environment",
                ));
            }
            tx.execute("INSERT INTO application_flags (environment, application_id, flag_key) VALUES (?1, ?2, ?3)", params![env, id, flag])?;
        }
        if before.as_ref().is_none_or(|app| {
            app.flags.iter().collect::<std::collections::BTreeSet<_>>() != selected
        }) {
            tx.execute(
                "UPDATE applications SET revision = revision + 1 WHERE id = ?1",
                [&id],
            )?;
        }
        let after = json!({"name": name, "flags": selected});
        let before = before
            .as_ref()
            .map(|app| json!({"name": app.name, "flags": app.flags}));
        if before.as_ref() != Some(&after) {
            audit(
                &tx,
                (actor, env),
                if created { "created" } else { "updated" },
                "app",
                &id,
                name,
                json!({"application_id": id, "before": before, "after": after}),
            )?;
        }
        if created {
            Self::insert_key(&tx, actor, env, &id, "Default")?;
        }
        let app = Self::read_application(&tx, env, &id)?;
        tx.commit()?;
        Ok(app)
    }
}
