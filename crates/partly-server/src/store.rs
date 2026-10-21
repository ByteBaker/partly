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
use serde::Serialize;
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
    sessions: HashMap<String, Instant>,
    lock: Option<File>,
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

fn audit(
    db: &Connection,
    env: &str,
    action: &str,
    kind: &str,
    id: &str,
    name: &str,
    details: Value,
) -> Result<(), ApiError> {
    db.execute("INSERT INTO audits (environment, actor, action, item_type, item_id, item_name, application_id, details) SELECT ?1, username, ?2, ?3, ?4, ?5, ?6, ?7 FROM admin WHERE id = 1",
        params![env, action, kind, id, name, details.get("application_id").and_then(Value::as_str), details.to_string()])?;
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
                let count: u32 =
                    connection
                        .query_row("SELECT COUNT(*) FROM admin WHERE id = 1", [], |r| r.get(0))?;
                let hash: String = connection.query_row(
                    "SELECT password_hash FROM admin WHERE id = 1",
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
                "INSERT INTO admin VALUES (1, ?1, ?2)",
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

    pub fn credentials(&self) -> Result<(String, String), ApiError> {
        Ok(self.db()?.query_row(
            "SELECT username, password_hash FROM admin WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }

    pub fn reset_password(&self, hash: &str) -> Result<(), ApiError> {
        self.db()?
            .execute("UPDATE admin SET password_hash = ?1 WHERE id = 1", [hash])?;
        Ok(())
    }

    pub fn new_session(&mut self) -> String {
        self.sessions.retain(|_, expiry| *expiry > Instant::now());
        // The POC has one admin; bound unused sessions without adding a session service.
        if self.sessions.len() >= 100 {
            self.sessions.clear();
        }
        let token = secret();
        self.sessions
            .insert(token.clone(), Instant::now() + Duration::from_secs(43_200));
        token
    }

    pub fn authenticated(&self, token: &str) -> bool {
        self.sessions
            .get(token)
            .is_some_and(|expiry| *expiry > Instant::now())
    }

    pub fn authorize(&self, token: &str) -> Result<(), ApiError> {
        if self.authenticated(token) {
            Ok(())
        } else {
            Err(ApiError::unauthorized())
        }
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
                env,
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

    pub fn delete_flag(&mut self, env: &str, key: &str) -> Result<(), ApiError> {
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
            env,
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
                env,
                if created { "created" } else { "updated" },
                "app",
                &id,
                name,
                json!({"application_id": id, "before": before, "after": after}),
            )?;
        }
        if created {
            Self::insert_key(&tx, env, &id, "Default")?;
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

    fn insert_key(db: &Connection, env: &str, app: &str, name: &str) -> Result<SdkKey, ApiError> {
        let name = key_name(name)?;
        let id = secret()[..16].to_owned();
        let key = format!("pk_{}", Alphanumeric.sample_string(&mut OsRng, 24));
        let masked = format!("{}••••••••••••{}", &key[..7], &key[key.len() - 4..]);
        let hash = format!("{:x}", Sha256::digest(key.as_bytes()));
        // shortcut: SDK keys are plaintext in the local POC, add protected storage before production use.
        db.execute("INSERT INTO sdk_keys (id, application_id, secret, secret_hash, masked_secret, name) VALUES (?1, ?2, ?3, ?4, ?5, ?6)", params![id, app, key, hash, masked, name])?;
        audit(
            db,
            env,
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

    pub fn create_key(&mut self, env: &str, app: &str, name: &str) -> Result<SdkKey, ApiError> {
        self.environment(env)?;
        let tx = self
            .connection
            .as_mut()
            .ok_or_else(|| ApiError::conflict("Complete setup first"))?
            .transaction()?;
        Self::read_application(&tx, env, app)?;
        let key = Self::insert_key(&tx, env, app, name)?;
        tx.commit()?;
        Ok(key)
    }

    pub fn change_key(&mut self, id: &str, name: Option<&str>) -> Result<(), ApiError> {
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
                    &env,
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
                &env,
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
