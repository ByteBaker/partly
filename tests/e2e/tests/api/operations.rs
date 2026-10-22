// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::support::*;
use reqwest::Method;
use serde_json::json;
use std::{
    fs,
    path::Path,
    process::{Output, Stdio},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

async fn command(args: &[&str], password: Option<&str>) -> Result<Output> {
    let mut child = Command::new(binary())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    if let Some(password) = password {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(password.as_bytes())
            .await?;
    }
    drop(child.stdin.take());
    Ok(tokio::time::timeout(Duration::from_secs(10), child.wait_with_output()).await??)
}

fn failed(output: &Output, message: &str) {
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(message),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

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
async fn setup_requires_local_binding_without_empty_files() -> Result {
    let mut server = Server::new()?;
    let db = server.db.to_str().unwrap();
    failed(
        &command(
            &["serve", "--host", "0.0.0.0", "--port", "0", "--db", db],
            None,
        )
        .await?,
        "setup locally first",
    );
    assert!(!server.db.exists() && !Path::new(&format!("{db}.lock")).exists());
    failed(
        &command(&["reset-password", "--db", db], Some("new-password-123\n")).await?,
        "does not exist",
    );
    assert!(!server.db.exists() && !Path::new(&format!("{db}.lock")).exists());
    server.start().await?;
    assert!(!server.db.exists() && !Path::new(&format!("{}.lock", server.db.display())).exists());
    server.setup().await?;
    server.stop().await?;
    server.host = "0.0.0.0".into();
    server.start().await?;
    assert_eq!(
        server
            .login("admin", PASSWORD)
            .await?
            .get("/api/status")
            .await?["authenticated"],
        true
    );
    Ok(())
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
    assert!(!command(
        &["reset-password", "--db", server.db.to_str().unwrap()],
        Some("replacement-password-123\n")
    )
    .await?
    .status
    .success());
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
async fn hard_linked_database_is_rejected_without_changes() -> Result {
    for running in [true, false] {
        let (mut server, admin) = fixture().await?;
        admin
            .flag(
                "development",
                "new_checkout",
                json!({"mode":"percentage","percentage":37}),
            )
            .await?;
        admin.key("development").await?;
        let original = fs::read(&server.db)?;
        let alias = server.directory.path().join("hardlink.db");
        fs::hard_link(&server.db, &alias)?;
        if !running {
            server.stop().await?;
        }
        for path in [&alias, &server.db] {
            failed(
                &command(
                    &["reset-password", "--db", path.to_str().unwrap()],
                    Some("replacement-password-123\n"),
                )
                .await?,
                "hard links",
            );
            failed(
                &command(
                    &["serve", "--port", "0", "--db", path.to_str().unwrap()],
                    None,
                )
                .await?,
                "hard links",
            );
            assert_eq!(fs::read(&server.db)?, original);
        }
        assert!(!Path::new(&format!("{}.lock", alias.display())).exists());
        if running {
            admin.get("/api/environments").await?;
            admin
                .post(
                    "/api/login",
                    json!({"username":"admin","password":PASSWORD}),
                    200,
                )
                .await?;
            server.stop().await?;
        }
        fs::remove_file(alias)?;
        assert!(command(
            &["reset-password", "--db", server.db.to_str().unwrap()],
            Some("replacement-password-123\n")
        )
        .await?
        .status
        .success());
        server.start().await?;
        let client = server.login("admin", "replacement-password-123").await?;
        assert_eq!(
            client.get("/api/environments/development/flags").await?["new_checkout"]["percentage"],
            37
        );
        assert_eq!(
            client.get("/api/environments/development/apps").await?["applications"][0]["keys"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
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
        assert!(!format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .contains(password.trim()));
        assert_eq!(fs::read(&server.db)?, original);
    }
    let invalid = server.directory.path().join("invalid.db");
    fs::write(&invalid, b"not a database")?;
    assert!(!command(
        &["reset-password", "--db", invalid.to_str().unwrap()],
        Some("replacement-password-123\n")
    )
    .await?
    .status
    .success());
    assert_eq!(fs::read(invalid)?, b"not a database");
    Ok(())
}

#[tokio::test]
async fn stopped_backup_restores_flags_keys_and_admin() -> Result {
    let (mut server, admin) = fixture().await?;
    let key = admin.key("development").await?;
    admin
        .put(
            &format!("/api/keys/{}", text(&key["id"])),
            json!({"name":"Retained application"}),
            204,
        )
        .await?;
    admin
        .flag(
            "development",
            "new_checkout",
            json!({"mode":"percentage","percentage":25}),
        )
        .await?;
    server.stop().await?;
    let backup = server.directory.path().join("backup.db");
    fs::copy(&server.db, &backup)?;
    server.start().await?;
    server
        .login("admin", PASSWORD)
        .await?
        .flag(
            "development",
            "new_checkout",
            json!({"mode":"percentage","percentage":75}),
        )
        .await?;
    server.stop().await?;
    let mut restored = Server::new()?;
    fs::copy(backup, &restored.db)?;
    restored.start().await?;
    let recovered = restored.login("admin", PASSWORD).await?;
    assert_eq!(
        recovered.get("/api/environments/development/flags").await?["new_checkout"],
        json!({"mode":"percentage","percentage":25})
    );
    let saved = recovered.get("/api/environments/development/apps").await?["applications"][0]
        ["keys"]
        .clone();
    assert_eq!(saved.as_array().unwrap().len(), 1);
    assert_eq!(saved[0]["name"], "Retained application");
    assert_eq!(saved[0]["secret"], key["secret"]);
    let policy = status(
        restored
            .api(false)?
            .request(Method::GET, "/api/policy")
            .headers(policy_headers(&key))
            .send()
            .await?,
        200,
    )
    .await?;
    assert_eq!(policy["flags"]["new_checkout"]["percentage"], 25);
    assert_eq!(
        server
            .database()?
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
        1
    );
    Ok(())
}

#[tokio::test]
async fn future_database_version_is_preserved() -> Result {
    let (mut server, _) = fixture().await?;
    server.stop().await?;
    server.database()?.pragma_update(None, "user_version", 2)?;
    let original = fs::read(&server.db)?;
    assert!(server
        .start()
        .await
        .unwrap_err()
        .to_string()
        .contains("schema"));
    assert_eq!(fs::read(&server.db)?, original);
    Ok(())
}

#[tokio::test]
async fn login_throttle_ignores_forwarded_address_and_preserves_policy() -> Result {
    let (server, admin) = fixture().await?;
    let key = admin.key("development").await?;
    admin
        .flag("development", "new_checkout", json!({"mode":"enabled"}))
        .await?;
    let attacker = server.api(true)?;
    for attempt in 0..5 {
        status(
            attacker
                .request(Method::POST, "/api/login")
                .header("X-Forwarded-For", format!("192.0.2.{attempt}"))
                .json(&json!({"username":"admin","password":"wrong-password"}))
                .send()
                .await?,
            401,
        )
        .await?;
    }
    let response = attacker
        .request(Method::POST, "/api/login")
        .header("X-Forwarded-For", "198.51.100.1")
        .json(&json!({"username":"admin","password":PASSWORD}))
        .send()
        .await?;
    assert_eq!(response.headers()["retry-after"], "60");
    assert_eq!(response.headers()["cache-control"], "no-store");
    status(response, 429).await?;
    status(
        attacker
            .request(Method::GET, "/api/policy")
            .headers(policy_headers(&key))
            .send()
            .await?,
        200,
    )
    .await?;
    admin.get("/api/environments").await?;
    Ok(())
}

#[tokio::test]
async fn concurrent_logins_and_policy_reads() -> Result {
    let (server, admin) = fixture().await?;
    let key = admin.key("development").await?;
    admin
        .flag("development", "new_checkout", json!({"mode":"enabled"}))
        .await?;
    let mut tasks = tokio::task::JoinSet::new();
    for index in 0..20 {
        let client = server.api(true)?;
        let headers = policy_headers(&key);
        tasks.spawn(async move {
            let login = index % 2 != 0;
            let request = if login {
                client
                    .request(Method::POST, "/api/login")
                    .json(&json!({"username":"admin","password":"incorrect-password"}))
            } else {
                client.request(Method::GET, "/api/policy").headers(headers)
            };
            Ok::<_, reqwest::Error>((login, request.send().await?.status().as_u16()))
        });
    }
    let mut throttled = false;
    while let Some(result) = tasks.join_next().await {
        let (login, code) = result??;
        if login {
            assert!([401, 429].contains(&code));
        } else {
            assert_eq!(code, 200);
        }
        throttled |= code == 429;
    }
    assert!(throttled);
    Ok(())
}

#[tokio::test]
async fn shutdown_releases_database_even_with_stalled_body() -> Result {
    for https in [false, true] {
        for stalled in [false, true] {
            for signal in ["INT", "TERM"] {
                let mut server = Server::new()?;
                let tls = Tls::new("127.0.0.1", false)?;
                if https {
                    tls.configure(&mut server);
                }
                server.start().await?;
                server
                    .setup()
                    .await?
                    .flag("development", "new_checkout", json!({"mode":"enabled"}))
                    .await?;
                let mut connection = if stalled {
                    Some(crate::support::connection(&server).await?)
                } else {
                    None
                };
                if let Some(stream) = connection.as_mut() {
                    stream.write_all(format!("POST /api/login HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nX-Partly-Request: dashboard\r\nContent-Type: application/json\r\nContent-Length: 1000\r\nExpect: 100-continue\r\n\r\n",server.port).as_bytes()).await?;
                    let mut buffer = [0; 4096];
                    let size =
                        tokio::time::timeout(Duration::from_secs(2), stream.read(&mut buffer))
                            .await??;
                    assert!(buffer[..size].starts_with(b"HTTP/1.1 100 Continue"));
                    stream.write_all(b"{").await?;
                }
                server.signal(signal).await?;
                drop(connection);
                server.start().await?;
                assert_eq!(
                    server
                        .login("admin", PASSWORD)
                        .await?
                        .get("/api/environments/development/flags")
                        .await?["new_checkout"],
                    json!({"mode":"enabled"})
                );
            }
        }
    }
    Ok(())
}
