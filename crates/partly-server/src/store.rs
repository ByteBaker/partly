// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::ApiError;
use argon2::PasswordHash;
use partly_core::{Flag, Policy, MAX_POLICY_BYTES};
use rand::{
    distributions::{Alphanumeric, DistString},
    rngs::OsRng,
    RngCore,
};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    fs::{File, OpenOptions, TryLockError},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub struct Store {
    path: PathBuf,
    connection: Option<Connection>,
    sessions: HashMap<String, (i64, Instant)>,
    lock: Option<File>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Viewer,
    Editor,
    Manager,
}

#[derive(Clone, Serialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub is_admin: bool,
    pub enabled: bool,
    pub permissions: BTreeMap<String, Role>,
}

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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserInput {
    pub username: String,
    pub is_admin: bool,
    pub enabled: bool,
    pub permissions: BTreeMap<String, Role>,
    pub password: Option<String>,
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

#[derive(Serialize)]
pub struct SdkKey {
    id: String,
    name: String,
    secret: Option<String>,
    masked_secret: String,
    created_at: String,
    revoked_at: Option<String>,
}

#[derive(Serialize)]
pub struct Application {
    id: String,
    name: String,
    flags: Vec<String>,
    keys: Vec<SdkKey>,
}

impl Application {
    pub fn hide_secrets(&mut self) {
        for key in &mut self.keys {
            key.secret = None;
        }
    }
}

fn audit(
    db: &Connection,
    scope: (&str, &str),
    action: &str,
    kind: &str,
    id: &str,
    name: &str,
    details: Value,
) -> Result<(), ApiError> {
    db.execute("INSERT INTO audits (environment, actor, action, item_type, item_id, item_name, application_id, details) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![scope.1, scope.0, action, kind, id, name, details.get("application_id").and_then(Value::as_str), details.to_string()])?;
    Ok(())
}

fn key_name(name: &str) -> Result<&str, ApiError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(ApiError::bad(
            "Use a name of 1–64 characters without control characters",
        ));
    }
    Ok(name)
}

fn check_policy_size(policy: &Policy) -> Result<(), ApiError> {
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

pub fn secret() -> String {
    let mut bytes = [0; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn lock_database(path: &Path) -> Result<File, ApiError> {
    let canonical = path
        .canonicalize()
        .map_err(|error| ApiError::internal(format!("Cannot resolve database path: {error}")))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(&canonical).map_err(|error| {
            ApiError::internal(format!("Cannot inspect database links: {error}"))
        })?;
        // Hard links share database data but use different lock and journal paths.
        if metadata.nlink() > 1 {
            return Err(ApiError::conflict(
                "Databases with hard links are unsupported; remove extra hard links or use an independent copy",
            ));
        }
    }
    let mut lock_path = canonical.into_os_string();
    lock_path.push(".lock");
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    // A separate persistent inode avoids interfering with SQLite's own database locks.
    let file = options
        .open(lock_path)
        .map_err(|error| ApiError::internal(format!("Cannot open database lock: {error}")))?;
    file.try_lock().map_err(|error| match error {
        TryLockError::WouldBlock => {
            ApiError::conflict("Database is in use; stop the Partly server before continuing")
        }
        TryLockError::Error(error) => ApiError::internal(format!("Cannot lock database: {error}")),
    })?;
    Ok(file)
}

impl Store {
    pub fn open(path: PathBuf) -> Result<Self, ApiError> {
        let mut lock = None;
        let connection = match path.try_exists() {
            Ok(false) => None,
            Ok(true) => {
                lock = Some(lock_database(&path)?);
                let connection =
                    Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
                let integrity: String =
                    connection.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
                let version: u32 = connection.query_row("PRAGMA user_version", [], |r| r.get(0))?;
                if integrity != "ok" || version != 1 {
                    return Err(ApiError::internal(
                        "Existing database is invalid or has an unsupported schema",
                    ));
                }
                for query in [
                    "SELECT id, username, password_hash, is_admin, enabled FROM users",
                    "SELECT user_id, environment, role FROM permissions",
                    "SELECT id, name, application_id, secret, secret_hash, masked_secret, created_at, revoked_at FROM sdk_keys",
                    "SELECT id, environment, name, revision FROM applications",
                    "SELECT environment, application_id, flag_key FROM application_flags",
                    "SELECT id, environment, actor, action, item_type, item_id, item_name, application_id, details, created_at FROM audits",
                    "SELECT environment, key, config FROM flags",
                ] {
                    connection.prepare(query).map_err(|_| {
                        ApiError::internal("Existing database has an incompatible development schema. Use a fresh --db path; the existing file has not been changed.")
                    })?;
                }
                if connection
                    .query_row("PRAGMA foreign_key_check", [], |_| Ok(()))
                    .optional()?
                    .is_some()
                {
                    return Err(ApiError::internal(
                        "Existing database has invalid references",
                    ));
                }
                let count: u32 = connection.query_row(
                    "SELECT COUNT(*) FROM users WHERE id = 1 AND is_admin = 1 AND enabled = 1",
                    [],
                    |r| r.get(0),
                )?;
                let hash: String = connection.query_row(
                    "SELECT password_hash FROM users WHERE id = 1 AND is_admin = 1 AND enabled = 1",
                    [],
                    |r| r.get(0),
                )?;
                if count != 1 || PasswordHash::new(&hash).is_err() {
                    return Err(ApiError::internal("Existing database setup is incomplete"));
                }
                Some(connection)
            }
            Err(e) => {
                return Err(ApiError::internal(format!(
                    "Cannot inspect database path: {e}"
                )))
            }
        };
        let store = Self {
            path,
            connection,
            sessions: HashMap::new(),
            lock,
        };
        if let Some(connection) = &store.connection {
            connection.execute_batch("PRAGMA foreign_keys = ON;")?;
            for env in ["development", "production"] {
                let policy = store.policy(env)?;
                policy.validate().map_err(ApiError::internal)?;
                check_policy_size(&policy)?;
            }
        }
        Ok(store)
    }

    pub fn ready(&self) -> bool {
        self.connection.is_some()
    }

    pub fn db(&self) -> Result<&Connection, ApiError> {
        self.connection
            .as_ref()
            .ok_or_else(|| ApiError::conflict("Complete setup first"))
    }

    pub fn setup(&mut self, username: &str, hash: &str) -> Result<(), ApiError> {
        if self.ready() {
            return Err(ApiError::conflict("Setup is already complete"));
        }
        if username.trim() != username || username.is_empty() || username.len() > 64 {
            return Err(ApiError::bad(
                "Use a username of 1–64 bytes without surrounding whitespace",
            ));
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(&self.path).map_err(|e| {
            ApiError::internal(format!(
                "Cannot create database; existing files are preserved: {e}"
            ))
        })?;
        let result = (|| -> Result<(Connection, File), ApiError> {
            let lock = lock_database(&self.path)?;
            let mut connection =
                Connection::open_with_flags(&self.path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
            connection.execute_batch("PRAGMA foreign_keys = ON;")?;
            let transaction = connection.transaction()?;
            transaction.execute_batch(include_str!("../migrations/001_initial.sql"))?;
            transaction.execute(
                "INSERT INTO users (id, username, password_hash, is_admin) VALUES (1, ?1, ?2, 1)",
                params![username, hash],
            )?;
            transaction.commit()?;
            Ok((connection, lock))
        })();
        match result {
            Ok((connection, lock)) => {
                self.connection = Some(connection);
                self.lock = Some(lock);
                Ok(())
            }
            Err(error) => {
                // Only remove the file this setup attempt exclusively created.
                drop(file);
                let _ = std::fs::remove_file(&self.path);
                Err(error)
            }
        }
    }

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

    pub fn reset_password(&self, hash: &str) -> Result<(), ApiError> {
        self.db()?
            .execute("UPDATE users SET password_hash = ?1 WHERE id = 1", [hash])?;
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

    pub fn key_environment(&self, id: &str) -> Result<String, ApiError> {
        self.db()?.query_row("SELECT a.environment FROM sdk_keys k JOIN applications a ON a.id = k.application_id WHERE k.id = ?1", [id], |r| r.get(0)).optional()?.ok_or_else(|| ApiError::not_found("Key not found"))
    }

    pub fn logout(&mut self, token: &str) {
        self.sessions.remove(token);
    }

    pub fn environment(&self, env: &str) -> Result<(), ApiError> {
        if !matches!(env, "development" | "production") {
            return Err(ApiError::not_found("Unknown environment"));
        }
        Ok(())
    }

    pub fn policy(&self, env: &str) -> Result<Policy, ApiError> {
        self.environment(env)?;
        Self::read_policy(self.db()?, env)
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
        let flags = rows
            .into_iter()
            .map(|(key, config)| {
                let flag = serde_json::from_str(&config)
                    .map_err(|_| ApiError::internal("Invalid stored flag configuration"))?;
                partly_core::validate_json_number_limits(config.as_bytes())
                    .map_err(|_| ApiError::internal("Invalid stored flag configuration"))?;
                Ok((key, flag))
            })
            .collect::<Result<BTreeMap<_, _>, ApiError>>()?;
        Ok(Policy {
            schema_version: 1,
            project,
            environment: env.into(),
            application_id: "environment".into(),
            revision,
            flags,
        })
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
        if !partly_core::valid_key(key) {
            return Err(ApiError::bad("Invalid flag key"));
        }
        flag.validate().map_err(ApiError::bad)?;
        let config =
            serde_json::to_string(flag).map_err(|_| ApiError::internal("Cannot encode flag"))?;
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
        let changed = transaction.execute("INSERT INTO flags (environment, key, config) VALUES (?1, ?2, ?3) ON CONFLICT(environment, key) DO UPDATE SET config = excluded.config WHERE config != excluded.config", params![env, key, config])?;
        if changed > 0 {
            transaction.execute(
                "UPDATE environments SET revision = revision + 1 WHERE name = ?1",
                [env],
            )?;
            transaction.execute("UPDATE applications SET revision = revision + 1 WHERE id IN (SELECT application_id FROM application_flags WHERE environment = ?1 AND flag_key = ?2)", [env, key])?;
            check_policy_size(&Self::read_policy(&transaction, env)?)?;
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
        Ok(())
    }

    pub fn delete_flag(&mut self, actor: &str, env: &str, key: &str) -> Result<(), ApiError> {
        self.environment(env)?;
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
        transaction.execute("UPDATE applications SET revision = revision + 1 WHERE id IN (SELECT application_id FROM application_flags WHERE environment = ?1 AND flag_key = ?2)", [env, key])?;
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
        Ok(())
    }

    fn flag_apps(db: &Connection, env: &str, key: &str) -> Result<Vec<Value>, ApiError> {
        let mut query = db.prepare("SELECT a.id, a.name FROM applications a JOIN application_flags f ON a.id = f.application_id WHERE f.environment = ?1 AND f.flag_key = ?2 ORDER BY a.name, a.id")?;
        let rows = query
            .query_map([env, key], |r| {
                Ok(json!({"id": r.get::<_, String>(0)?, "name": r.get::<_, String>(1)?}))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn flag_impact(&self, env: &str, key: &str) -> Result<Vec<Value>, ApiError> {
        self.environment(env)?;
        if !self.policy(env)?.flags.contains_key(key) {
            return Err(ApiError::not_found("Flag not found"));
        }
        Self::flag_apps(self.db()?, env, key)
    }

    fn read_application(db: &Connection, env: &str, id: &str) -> Result<Application, ApiError> {
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

    fn read_keys(db: &Connection, application: &str) -> Result<Vec<SdkKey>, ApiError> {
        let mut query = db.prepare("SELECT id, name, secret, masked_secret, created_at, revoked_at FROM sdk_keys WHERE application_id = ?1 ORDER BY rowid")?;
        let rows = query
            .query_map([application], |r| {
                Ok(SdkKey {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    secret: r.get(2)?,
                    masked_secret: r.get(3)?,
                    created_at: r.get(4)?,
                    revoked_at: r.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn insert_key(
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
        db.execute("INSERT INTO sdk_keys (id, application_id, secret, secret_hash, masked_secret, name) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![id, app, key, hash, masked, name])?;
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
        let (app, env, old_name, revoked): (String, String, String, Option<String>) = tx.query_row("SELECT a.id, a.environment, k.name, k.revoked_at FROM sdk_keys k JOIN applications a ON a.id = k.application_id WHERE k.id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).optional()?.ok_or_else(|| ApiError::not_found("Key not found"))?;
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
        let hash = format!("{:x}", Sha256::digest(key.as_bytes()));
        let (scope_env, scope_app, revoked, revision): (String, String, Option<String>, i64) = self.db()?.query_row("SELECT a.environment, a.id, k.revoked_at, a.revision FROM sdk_keys k JOIN applications a ON a.id = k.application_id WHERE k.secret_hash = ?1", [hash], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).optional()?.ok_or_else(ApiError::unauthorized)?;
        if revoked.is_some() {
            return Err(ApiError(
                axum::http::StatusCode::UNAUTHORIZED,
                "key_revoked".into(),
            ));
        }
        if env != scope_env || app != scope_app {
            return Err(ApiError::unauthorized());
        }
        let mut query = self.db()?.prepare(
            "SELECT flag_key FROM application_flags WHERE application_id = ?1 ORDER BY flag_key",
        )?;
        let flags = query
            .query_map([app], |r| r.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        let mut policy = self.policy(env)?;
        policy.application_id = app.into();
        policy
            .flags
            .retain(|key, _| flags.binary_search(key).is_ok());
        policy.revision = u64::try_from(revision)
            .map_err(|_| ApiError::internal("Invalid application revision"))?;
        Ok(policy)
    }

    pub fn audits(
        &self,
        env: &str,
        before: Option<i64>,
        action: Option<&str>,
        app: Option<&str>,
    ) -> Result<Vec<Value>, ApiError> {
        self.environment(env)?;
        let mut query = self.db()?.prepare("SELECT id, actor, action, item_type, item_id, item_name, details, created_at FROM audits WHERE environment = ?1 AND (?2 IS NULL OR id < ?2) AND (?3 IS NULL OR action = ?3) AND (?4 IS NULL OR application_id = ?4 OR EXISTS (SELECT 1 FROM json_each(details, '$.applications') WHERE json_extract(value, '$.id') = ?4)) ORDER BY id DESC LIMIT 100")?;
        let rows = query.query_map(params![env, before, action, app], |r| {
            let raw: String = r.get(6)?;
            let details: Value = serde_json::from_str(&raw).map_err(|e| rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, Box::new(e)))?;
            Ok(json!({"id": r.get::<_, i64>(0)?, "actor": r.get::<_, String>(1)?, "action": r.get::<_, String>(2)?, "item_type": r.get::<_, String>(3)?, "item_id": r.get::<_, String>(4)?, "item_name": r.get::<_, String>(5)?, "details": details, "created_at": r.get::<_, String>(7)?}))
        })?.collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_hierarchy_is_environment_scoped_and_disabled_users_have_no_access() {
        for role in [Role::Viewer, Role::Editor, Role::Manager] {
            let mut user = User {
                id: 2,
                username: "teammate".into(),
                enabled: true,
                is_admin: false,
                permissions: BTreeMap::from([("development".into(), role)]),
            };
            assert!(user.allows("development", Role::Viewer));
            assert_eq!(
                user.allows("development", Role::Editor),
                role != Role::Viewer
            );
            assert_eq!(
                user.allows("development", Role::Manager),
                role == Role::Manager
            );
            assert!(!user.allows("production", Role::Viewer));
            assert!(user.require_admin().is_err());
            user.is_admin = true;
            assert!(user.allows("production", Role::Manager));
            user.enabled = false;
            assert!(!user.allows("development", Role::Viewer));
            assert!(!user.allows("production", Role::Manager));
            assert!(user.require_admin().is_err());
        }
    }
}
