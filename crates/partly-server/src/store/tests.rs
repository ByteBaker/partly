// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;
use std::collections::BTreeMap;

#[test]
fn application_policy_reads_only_selected_flag_configurations() {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(include_str!("../../migrations/001_initial.sql"))
        .unwrap();
    db.execute_batch("INSERT INTO applications (id, environment, name) VALUES ('app_test', 'development', 'Test'); INSERT INTO flags VALUES ('development', 'selected', '{\"mode\":\"enabled\"}'); INSERT INTO application_flags VALUES ('development', 'app_test', 'selected');")
        .unwrap();
    // An invalid unrelated regex proves its configuration was not decoded at all.
    db.execute(
        "INSERT INTO flags VALUES ('development', 'unrelated', ?1)",
        [r#"{"mode":"custom","action":"enable","groups":[[{"k":"tag","op":"regex","v":"["}]]}"#],
    )
    .unwrap();
    let policy = Store::read_policy(&db, "development", Some("app_test")).unwrap();
    assert_eq!(
        policy.flags.keys().map(String::as_str).collect::<Vec<_>>(),
        ["selected"]
    );
    assert!(Store::read_policy(&db, "development", None).is_err());
    db.execute(
        "INSERT INTO application_flags VALUES ('development', 'app_test', 'unrelated')",
        [],
    )
    .unwrap();
    assert!(Store::read_policy(&db, "development", Some("app_test")).is_err());
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
