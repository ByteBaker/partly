// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::{
    Store,
    audits::audit,
    models::{Role, User, UserInput},
    secret,
};
use crate::ApiError;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

impl User {
    pub fn allows(&self, env: &str, role: Role) -> bool {
        self.enabled
            && (self.is_admin
                || self
                    .permissions
                    .get(env)
                    .is_some_and(|assigned| *assigned >= role))
    }
    pub fn require(&self, env: &str, role: Role) -> Result<(), ApiError> {
        if self.allows(env, role) {
            Ok(())
        } else {
            Err(ApiError::forbidden())
        }
    }
    pub fn require_admin(&self) -> Result<(), ApiError> {
        if self.enabled && self.is_admin {
            Ok(())
        } else {
            Err(ApiError::forbidden())
        }
    }
}

impl UserInput {
    pub fn validate(&self) -> Result<(), ApiError> {
        if self.username.trim() != self.username
            || self.username.is_empty()
            || self.username.len() > 64
            || self.username.chars().any(char::is_control)
        {
            return Err(ApiError::bad(
                "Use a username of 1–64 bytes without control characters or surrounding whitespace",
            ));
        }
        if self
            .permissions
            .keys()
            .any(|env| !matches!(env.as_str(), "development" | "production"))
        {
            return Err(ApiError::bad("Unknown permission environment"));
        }
        if self.is_admin && !self.permissions.is_empty() {
            return Err(ApiError::bad(
                "Administrators already have access to all environments",
            ));
        }
        if let Some(password) = &self.password {
            crate::auth::validate_password(password)?;
        }
        Ok(())
    }
}

impl Store {
    pub fn credentials(&self, username: &str) -> Result<(i64, String, bool), ApiError> {
        // Verify an unknown login against the initial administrator's hash as well.
        let found = self
            .db()?
            .query_row(
                "SELECT id, password_hash, enabled FROM users WHERE username = ?1",
                [username],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some(credentials) = found {
            return Ok(credentials);
        }
        let hash =
            self.db()?
                .query_row("SELECT password_hash FROM users WHERE id = 1", [], |r| {
                    r.get(0)
                })?;
        Ok((0, hash, false))
    }

    pub fn reset_password(&mut self, hash: &str) -> Result<(), ApiError> {
        let user = Self::read_user(self.db()?, 1)?;
        self.save_user(
            "offline recovery",
            Some(user.id),
            &UserInput {
                username: user.username,
                password: None,
                is_admin: user.is_admin,
                enabled: user.enabled,
                permissions: user.permissions,
            },
            Some(hash),
        )?;
        Ok(())
    }

    pub fn new_session(&mut self, user_id: i64) -> String {
        self.sessions
            .retain(|_, (_, expiry)| *expiry > Instant::now());
        if self.sessions.len() >= 100 {
            if let Some(oldest) = self
                .sessions
                .iter()
                .min_by_key(|(_, (_, expiry))| *expiry)
                .map(|(key, _)| key.clone())
            {
                self.sessions.remove(&oldest);
            }
        }
        let token = secret();
        self.sessions.insert(
            token.clone(),
            (user_id, Instant::now() + Duration::from_secs(43_200)),
        );
        token
    }

    pub fn authenticated(&self, token: &str) -> bool {
        self.authorize(token).is_ok()
    }

    pub fn authorize(&self, token: &str) -> Result<User, ApiError> {
        let (id, expiry) = self
            .sessions
            .get(token)
            .ok_or_else(ApiError::unauthorized)?;
        if *expiry <= Instant::now() {
            return Err(ApiError::unauthorized());
        }
        let user = Self::read_user(self.db()?, *id)?;
        if !user.enabled {
            return Err(ApiError::unauthorized());
        }
        Ok(user)
    }

    fn read_user(db: &Connection, id: i64) -> Result<User, ApiError> {
        let mut user = db
            .query_row(
                "SELECT id, username, is_admin, enabled FROM users WHERE id = ?1",
                [id],
                |r| {
                    Ok(User {
                        id: r.get(0)?,
                        username: r.get(1)?,
                        is_admin: r.get(2)?,
                        enabled: r.get(3)?,
                        permissions: BTreeMap::new(),
                    })
                },
            )
            .optional()?
            .ok_or_else(|| ApiError::not_found("User not found"))?;
        let mut query =
            db.prepare("SELECT environment, role FROM permissions WHERE user_id = ?1")?;
        let rows = query
            .query_map([id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        for (env, role) in rows {
            let role = match role.as_str() {
                "viewer" => Role::Viewer,
                "editor" => Role::Editor,
                "manager" => Role::Manager,
                _ => return Err(ApiError::internal("Invalid stored permission")),
            };
            user.permissions.insert(env, role);
        }
        Ok(user)
    }

    pub fn users(&self) -> Result<Vec<User>, ApiError> {
        let db = self.db()?;
        let mut query = db.prepare("SELECT id FROM users ORDER BY username")?;
        let ids = query
            .query_map([], |r| r.get(0))?
            .collect::<Result<Vec<i64>, _>>()?;
        ids.into_iter().map(|id| Self::read_user(db, id)).collect()
    }

    pub fn save_user(
        &mut self,
        actor: &str,
        id: Option<i64>,
        input: &UserInput,
        hash: Option<&str>,
    ) -> Result<User, ApiError> {
        input.validate()?;
        if id == Some(1) && (!input.enabled || !input.is_admin) {
            return Err(ApiError::conflict(
                "The initial administrator must remain enabled with administrator access",
            ));
        }
        if id.is_none() && hash.is_none() {
            return Err(ApiError::bad("A new user requires a password"));
        }
        let tx = self
            .connection
            .as_mut()
            .ok_or_else(|| ApiError::conflict("Complete setup first"))?
            .transaction()?;
        let before = id.map(|id| Self::read_user(&tx, id)).transpose()?;
        if before
            .as_ref()
            .is_some_and(|user| user.username != input.username)
        {
            return Err(ApiError::bad("Usernames cannot be changed"));
        }
        if let Some(user) = &before {
            if hash.is_none()
                && user.is_admin == input.is_admin
                && user.enabled == input.enabled
                && user.permissions == input.permissions
            {
                return Ok(user.clone());
            }
        }
        let duplicate: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM users WHERE username = ?1 AND (?2 IS NULL OR id != ?2))",
            params![input.username, id],
            |r| r.get(0),
        )?;
        if duplicate {
            return Err(ApiError::conflict("Username already exists"));
        }
        let id = match id {
            Some(id) => {
                tx.execute("UPDATE users SET is_admin = ?1, enabled = ?2, password_hash = COALESCE(?3, password_hash) WHERE id = ?4", params![input.is_admin, input.enabled, hash, id])?;
                tx.execute("DELETE FROM permissions WHERE user_id = ?1", [id])?;
                id
            }
            None => {
                tx.execute("INSERT INTO users (username, password_hash, is_admin, enabled) VALUES (?1, ?2, ?3, ?4)", params![input.username, hash, input.is_admin, input.enabled])?;
                tx.last_insert_rowid()
            }
        };
        for (env, role) in &input.permissions {
            let role = match role {
                Role::Viewer => "viewer",
                Role::Editor => "editor",
                Role::Manager => "manager",
            };
            tx.execute(
                "INSERT INTO permissions VALUES (?1, ?2, ?3)",
                params![id, env, role],
            )?;
        }
        let saved = Self::read_user(&tx, id)?;
        for env in ["development", "production"] {
            let state = |user: &User| json!({"name": user.username, "is_admin": user.is_admin, "enabled": user.enabled, "role": user.permissions.get(env)});
            audit(
                &tx,
                (actor, env),
                if before.is_some() {
                    "updated"
                } else {
                    "created"
                },
                "user",
                &id.to_string(),
                &saved.username,
                json!({"before": before.as_ref().map(state), "after": state(&saved), "password_changed": hash.is_some()}),
            )?;
        }
        tx.commit()?;
        self.sessions.retain(|_, (owner, _)| *owner != id);
        Ok(saved)
    }

    pub fn logout(&mut self, token: &str) {
        self.sessions.remove(token);
    }
}
