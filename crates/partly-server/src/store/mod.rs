// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

mod applications;
mod audits;
mod database;
mod flags;
mod keys;
mod models;
#[cfg(test)]
mod tests;
mod usage;
mod users;

pub(crate) use models::{
    AppReference, Application, AuditEntry, FlagUsage, Role, SdkKey, UsageInput, User, UserInput,
};
use rand::{RngCore, rngs::OsRng};
use rusqlite::Connection;
use std::{collections::HashMap, fs::File, path::PathBuf, time::Instant};

pub struct Store {
    path: PathBuf,
    connection: Option<Connection>,
    sessions: HashMap<String, (i64, Instant)>,
    lock: Option<File>,
}

pub fn secret() -> String {
    let mut bytes = [0; 32];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
