// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::support::*;
use reqwest::Method;
use serde_json::json;
use std::{fs, path::Path};

use crate::support::cli::{command, failed};

fn records(server: &Server) -> Result<Vec<Vec<Vec<rusqlite::types::Value>>>> {
    let db = server.database()?;
    ["projects", "environments", "flags", "sdk_keys"]
        .iter()
        .map(|table| {
            let mut statement = db.prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))?;
            let columns = statement.column_count();
            let rows = statement
                .query_map([], |row| {
                    (0..columns).map(|column| row.get(column)).collect()
                })?
                .collect::<std::result::Result<_, _>>()?;
            Ok(rows)
        })
        .collect()
}

#[tokio::test]
async fn offline_reset_preserves_data_and_audits_recovery() -> Result {
    let (mut server, admin) = fixture().await?;
    admin
        .flag(
            "development",
            "new_checkout",
            json!({"mode":"percentage","percentage":37}),
        )
        .await?;
    let key = admin.key("development").await?;
    let before = records(&server)?;
    let mut audits = vec![];
    for env in ["development", "production"] {
        audits.push(
            admin
                .get(&format!("/api/environments/{env}/audits"))
                .await?["audits"]
                .clone(),
        );
    }
    let db = server.db.to_str().unwrap().to_owned();
    let password = "replacement-password-123\n";
    failed(
        &command(&["reset-password", "--db", &db], Some(password)).await?,
        "Database is in use",
    );
    failed(
        &command(&["serve", "--port", "0", "--db", &db], None).await?,
        "Database is in use",
    );
    let alias = server.directory.path().join("alias.db");
    std::os::unix::fs::symlink(&server.db, &alias)?;
    failed(
        &command(
            &["reset-password", "--db", alias.to_str().unwrap()],
            Some(password),
        )
        .await?,
        "Database is in use",
    );
    assert!(!Path::new(&format!("{}.lock", alias.display())).exists());
    server.stop().await?;
    assert!(fs::read(format!("{db}.lock"))?.is_empty());
    let output = command(&["reset-password", "--db", &db], Some(password)).await?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for bytes in [&output.stdout, &output.stderr] {
        assert!(!String::from_utf8_lossy(bytes).contains(password.trim()));
    }
    assert_eq!(records(&server)?, before);
    let database = server.database()?;
    let (username, hash): (String, String) =
        database.query_row("SELECT username,password_hash FROM users", [], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?;
    assert_eq!(username, "admin");
    assert!(hash.starts_with("$argon2id$") && !hash.contains(password.trim()));
    assert_eq!(
        database.query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))?,
        1
    );
    drop(database);
    server.start().await?;
    status(
        admin
            .request(Method::GET, "/api/environments")
            .send()
            .await?,
        401,
    )
    .await?;
    admin
        .post(
            "/api/login",
            json!({"username":"admin","password":PASSWORD}),
            401,
        )
        .await?;
    admin
        .post(
            "/api/login",
            json!({"username":"admin","password":password.trim()}),
            200,
        )
        .await?;
    for (env, previous) in ["development", "production"].iter().zip(audits) {
        let entries = admin
            .get(&format!("/api/environments/{env}/audits"))
            .await?["audits"]
            .clone();
        assert_eq!(
            &entries.as_array().unwrap()[1..],
            previous.as_array().unwrap()
        );
        let entry = &entries[0];
        assert_eq!(
            (
                &entry["actor"],
                &entry["action"],
                &entry["item_type"],
                &entry["item_name"]
            ),
            (
                &json!("offline recovery"),
                &json!("updated"),
                &json!("user"),
                &json!("admin")
            )
        );
        assert_eq!(entry["details"]["password_changed"], true);
        assert_eq!(entry["details"]["before"], entry["details"]["after"]);
        assert!(
            !entries.to_string().contains(password.trim())
                && !entries.to_string().contains("$argon2")
        );
    }
    let policy = status(
        server
            .api(false)?
            .request(Method::GET, "/api/policy")
            .headers(policy_headers(&key))
            .send()
            .await?,
        200,
    )
    .await?;
    assert_eq!(policy["flags"]["new_checkout"]["percentage"], 37);
    assert!(!server.log().contains(password.trim()));
    Ok(())
}

#[tokio::test]
async fn reset_rolls_back_when_audit_write_fails() -> Result {
    let (mut server, admin) = fixture().await?;
    server.stop().await?;
    let db = server.database()?;
    let hash: String = db.query_row("SELECT password_hash FROM users WHERE id = 1", [], |r| {
        r.get(0)
    })?;
    let count: i64 = db.query_row("SELECT COUNT(*) FROM audits", [], |r| r.get(0))?;
    db.execute_batch("CREATE TRIGGER reject_recovery_audit BEFORE INSERT ON audits WHEN NEW.environment = 'production' BEGIN SELECT RAISE(ABORT, 'test audit failure'); END")?;
    assert!(
        !command(
            &["reset-password", "--db", server.db.to_str().unwrap()],
            Some("replacement-password-123\n")
        )
        .await?
        .status
        .success()
    );
    assert_eq!(
        db.query_row("SELECT password_hash FROM users WHERE id = 1", [], |r| {
            r.get::<_, String>(0)
        })?,
        hash
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM audits", [], |r| r.get::<_, i64>(0))?,
        count
    );
    drop(db);
    server.start().await?;
    admin
        .post(
            "/api/login",
            json!({"username":"admin","password":PASSWORD}),
            200,
        )
        .await?;
    Ok(())
}

#[tokio::test]
async fn invalid_reset_preserves_database() -> Result {
    let (mut server, _) = fixture().await?;
    server.stop().await?;
    let original = fs::read(&server.db)?;
    for password in [
        "short\n".into(),
        "x".repeat(1025),
        "first-password\nsecond-password\n".into(),
    ] {
        let output = command(
            &["reset-password", "--db", server.db.to_str().unwrap()],
            Some(&password),
        )
        .await?;
        assert!(!output.status.success());
        assert!(
            !format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
            .contains(password.trim())
        );
        assert_eq!(fs::read(&server.db)?, original);
    }
    let invalid = server.directory.path().join("invalid.db");
    fs::write(&invalid, b"not a database")?;
    assert!(
        !command(
            &["reset-password", "--db", invalid.to_str().unwrap()],
            Some("replacement-password-123\n")
        )
        .await?
        .status
        .success()
    );
    assert_eq!(fs::read(invalid)?, b"not a database");
    Ok(())
}
