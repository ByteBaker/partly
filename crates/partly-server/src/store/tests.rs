// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;
use crate::ApiError;
use partly::evaluator::Flag;
use serde_json::json;
use std::collections::BTreeMap;

fn memory_store() -> Result<Store, ApiError> {
    let connection = Connection::open_in_memory()?;
    connection.execute_batch(include_str!("../../migrations/001_initial.sql"))?;
    Ok(Store {
        path: PathBuf::new(),
        connection: Some(connection),
        sessions: HashMap::new(),
        lock: None,
        policies: Default::default(),
    })
}

#[test]
fn failed_writes_preserve_cached_policy_and_committed_changes_replace_it() -> Result<(), ApiError> {
    let mut store = memory_store()?;
    let flag: Flag = serde_json::from_value(json!({"mode":"typed","result_type":"string","selected":"yes","other":"no","rollout":{"mode":"enabled"}})).unwrap();
    store.save_flag("admin", "development", "flag", &flag, true)?;
    let app = store.save_application("admin", "development", None, "App", &["flag".into()])?;
    let secret = app.keys[0].secret.as_deref().unwrap();
    let before = serde_json::to_value(store.app_policy(secret, "development", &app.id)?).unwrap();
    let mut changed = flag;
    if let Flag::Typed { rollout, .. } = &mut changed {
        **rollout = Flag::Disabled {};
    }
    store.db()?.execute_batch("CREATE TRIGGER reject_audit BEFORE INSERT ON audits BEGIN SELECT RAISE(ABORT, 'test failure'); END;")?;
    assert!(
        store
            .save_flag("admin", "development", "flag", &changed, false)
            .is_err()
    );
    assert_eq!(
        serde_json::to_value(store.app_policy(secret, "development", &app.id)?).unwrap(),
        before
    );
    store.db()?.execute_batch("DROP TRIGGER reject_audit;")?;
    store.save_flag("admin", "development", "flag", &changed, false)?;
    let saved = store.app_policy(secret, "development", &app.id)?;
    assert_eq!(
        serde_json::to_value(&saved.flags["flag"]).unwrap(),
        serde_json::to_value(changed).unwrap()
    );
    assert_eq!(saved.schema_version, 2);
    store.delete_flag("admin", "development", "flag")?;
    let deleted = store.app_policy(secret, "development", &app.id)?;
    assert!(deleted.flags.is_empty());
    assert_eq!(deleted.schema_version, 1);
    Ok(())
}

#[test]
fn repeated_reads_use_the_cached_revision_without_decoding_rules() -> Result<(), ApiError> {
    let mut store = memory_store()?;
    store.save_flag("admin", "development", "flag", &Flag::Enabled {}, true)?;
    let app = store.save_application("admin", "development", None, "App", &["flag".into()])?;
    // An invalid stored regex detects read-time compilation; only this test bypasses writes.
    store.db()?.execute(
        "UPDATE flags SET config = ?1",
        [r#"{"mode":"custom","action":"enable","groups":[[{"k":"id","op":"regex","v":"["}]]}"#],
    )?;
    assert!(store.policy("development")?.flags.contains_key("flag"));
    assert!(
        store
            .app_policy(
                app.keys[0].secret.as_deref().unwrap(),
                "development",
                &app.id
            )?
            .flags
            .contains_key("flag")
    );
    store.db()?.execute(
        "UPDATE environments SET revision = revision + 1 WHERE name = 'development'",
        [],
    )?;
    assert!(store.policy("development").is_err());
    Ok(())
}

#[test]
fn aggregate_regex_limit_rolls_back_the_flag_revision_and_audit() -> Result<(), ApiError> {
    let mut store = memory_store()?;
    let flag: Flag = serde_json::from_value(
        json!({"mode":"custom","action":"enable","groups":[[{"k":"id","op":"regex","v":"^user"}]]}),
    )
    .unwrap();
    for i in 0..64 {
        store.save_flag("admin", "development", &format!("f_{i}"), &flag, true)?;
    }
    let before = serde_json::to_value(store.policy("development")?).unwrap();
    assert!(
        store
            .save_flag("admin", "development", "too_many", &flag, true)
            .unwrap_err()
            .1
            .contains("64 regex")
    );
    assert_eq!(
        serde_json::to_value(store.policy("development")?).unwrap(),
        before
    );
    let count: i64 = store
        .db()?
        .query_row("SELECT COUNT(*) FROM audits", [], |row| row.get(0))?;
    assert_eq!(count, 64);
    assert!(!store.db()?.query_row(
        "SELECT EXISTS(SELECT 1 FROM flags WHERE key = 'too_many')",
        [],
        |row| row.get::<_, bool>(0)
    )?);
    Ok(())
}

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
