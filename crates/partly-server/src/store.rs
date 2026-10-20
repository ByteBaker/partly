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
    secret: String,
}

fn key_name(name: &str) -> Result<&str, ApiError> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(ApiError::bad(
            "Use a key name of 1–64 characters without control characters",
        ));
    }
    Ok(name)
}

fn check_policy_size(policy: &Policy) -> Result<(), ApiError> {
    let size = serde_json::to_vec(policy)
        .map_err(|_| ApiError::internal("Cannot encode policy"))?
        .len();
    if size > MAX_POLICY_BYTES {
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
                for query in [
                    "SELECT id, name, environment, secret FROM sdk_keys",
                    "SELECT environment, key, config FROM flags",
                ] {
                    connection.prepare(query).map_err(|_| {
                        ApiError::internal("Existing database has an incompatible development schema. Use a fresh --db path; the existing file has not been changed.")
                    })?;
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
            check_policy_size(&Self::read_policy(&transaction, env)?)?;
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
        transaction.commit()?;
        Ok(())
    }

    pub fn keys(&self, env: &str) -> Result<Vec<SdkKey>, ApiError> {
        self.environment(env)?;
        let mut query = self.db()?.prepare(
            "SELECT id, name, secret FROM sdk_keys WHERE environment = ?1 ORDER BY rowid",
        )?;
        let rows = query
            .query_map([env], |r| {
                Ok(SdkKey {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    secret: r.get(2)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn create_key(&self, env: &str, name: &str) -> Result<SdkKey, ApiError> {
        self.environment(env)?;
        let name = key_name(name)?.to_owned();
        let id = secret()[..16].to_owned();
        let key = format!("pk_{}", Alphanumeric.sample_string(&mut OsRng, 24));
        // shortcut: SDK keys are plaintext in the local POC, add protected storage before production use.
        self.db()?.execute(
            "INSERT INTO sdk_keys (id, environment, secret, name) VALUES (?1, ?2, ?3, ?4)",
            params![id, env, key, name],
        )?;
        Ok(SdkKey {
            id,
            name,
            secret: key,
        })
    }

    pub fn rename_key(&self, id: &str, name: &str) -> Result<(), ApiError> {
        let name = key_name(name)?;
        if self.db()?.execute(
            "UPDATE sdk_keys SET name = ?1 WHERE id = ?2",
            params![name, id],
        )? == 0
        {
            return Err(ApiError::not_found("Key not found"));
        }
        Ok(())
    }

    pub fn revoke_key(&self, id: &str) -> Result<(), ApiError> {
        if self
            .db()?
            .execute("DELETE FROM sdk_keys WHERE id = ?1", [id])?
            == 0
        {
            return Err(ApiError::not_found("Key not found"));
        }
        Ok(())
    }

    pub fn key_environment(&self, key: &str) -> Result<String, ApiError> {
        self.db()?
            .query_row(
                "SELECT environment FROM sdk_keys WHERE secret = ?1",
                [key],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(ApiError::unauthorized)
    }
}
