// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::{Store, flags::check_policy_size};
use crate::ApiError;
use argon2::PasswordHash;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions, TryLockError},
    path::{Path, PathBuf},
};

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
                    "SELECT id, environment, actor, action, item_type, item_id, item_name, \
            application_id, details, created_at FROM audits",
                    "SELECT environment, key, config FROM flags",
                    "SELECT environment, name, definition, deleted FROM result_types",
                    "SELECT api_key_id, application_id, environment, flag_key, last_used_at FROM flag_usage",
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
                )));
            }
        };
        let store = Self {
            path,
            connection,
            sessions: HashMap::new(),
            lock,
            policies: Default::default(),
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
            transaction.execute_batch(include_str!("../../migrations/001_initial.sql"))?;
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
}
