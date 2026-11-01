// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::{
    Store,
    models::{FlagUsage, UsageInput},
};
use crate::ApiError;
use rusqlite::params;

impl Store {
    pub fn record_usage(
        &mut self,
        key: &str,
        env: &str,
        app: &str,
        mut input: UsageInput,
    ) -> Result<(), ApiError> {
        let tx = self
            .connection
            .as_mut()
            .ok_or_else(|| ApiError::conflict("Complete setup first"))?
            .transaction()?;
        let (id, _) = Self::authorize_key(&tx, key, env, app)?;
        if input.flags.len() > 128 || input.flags.iter().any(|key| !partly_core::valid_key(key)) {
            return Err(ApiError::bad("Supply at most 128 valid flag names"));
        }
        input.flags.sort_unstable();
        input.flags.dedup();
        let timestamp: String =
            tx.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |r| {
                r.get(0)
            })?;
        {
            // Stale clients may report a removed selection; acknowledge it without recording it.
            let mut insert = tx.prepare_cached(
                "INSERT INTO flag_usage (api_key_id, application_id, environment, flag_key, \
            last_used_at) SELECT ?1, application_id, environment, flag_key, ?2 FROM \
            application_flags WHERE application_id = ?3 AND environment = ?4 AND flag_key = ?5 \
            ON CONFLICT (api_key_id, flag_key) DO UPDATE SET last_used_at = \
            MAX(flag_usage.last_used_at, excluded.last_used_at)",
            )?;
            for flag in &input.flags {
                insert.execute(params![id, timestamp, app, env, flag])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn flag_usage(&self, env: &str) -> Result<Vec<FlagUsage>, ApiError> {
        self.environment(env)?;
        let mut query = self.db()?.prepare(
            "SELECT u.api_key_id, k.name, u.application_id, a.name, u.flag_key, u.last_used_at \
            FROM flag_usage u JOIN sdk_keys k ON k.id = u.api_key_id JOIN applications a ON a.id \
            = u.application_id WHERE u.environment = ?1 ORDER BY u.flag_key, a.name, k.name, \
            u.api_key_id",
        )?;
        let rows = query
            .query_map([env], |r| {
                Ok(FlagUsage {
                    api_key_id: r.get(0)?,
                    key_name: r.get(1)?,
                    application_id: r.get(2)?,
                    application_name: r.get(3)?,
                    flag_key: r.get(4)?,
                    last_used_at: r.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}
