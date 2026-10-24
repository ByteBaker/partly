// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::support::*;
use reqwest::Method;
use rusqlite::params;
use serde_json::{json, Value};

async fn report(api: &Api, key: &Value, body: Value, expected: u16) -> Result {
    status(
        api.request(Method::POST, "/api/usage")
            .headers(policy_headers(key))
            .json(&body)
            .send()
            .await?,
        expected,
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn reports_deduplicate_without_changing_policy_or_audits() -> Result {
    let (server, admin) = fixture().await?;
    for name in ["enabled", "disabled", "untouched", "unassigned"] {
        admin
            .flag(
                "development",
                name,
                json!({"mode":if name == "enabled" { "enabled" } else { "disabled" }}),
            )
            .await?;
    }
    let app = admin
        .app(
            "development",
            "Checkout",
            &["enabled", "disabled", "untouched"],
        )
        .await?;
    assert!(app["keys"][0]["last_used_at"].is_null());
    let key = app_key(&app, "development");
    let sdk = server.api(false)?;
    let response = sdk
        .request(Method::GET, "/api/policy")
        .headers(policy_headers(&key))
        .send()
        .await?;
    let etag = response.headers()["etag"].clone();
    let policy = status(response, 200).await?;
    let audits = admin.get("/api/environments/development/audits").await?;
    let db = server.database()?;
    db.execute_batch("CREATE TABLE usage_writes (flag_key TEXT); CREATE TRIGGER count_usage_insert AFTER INSERT ON flag_usage BEGIN INSERT INTO usage_writes VALUES (NEW.flag_key); END; CREATE TRIGGER count_usage_update AFTER UPDATE ON flag_usage BEGIN INSERT INTO usage_writes VALUES (NEW.flag_key); END;")?;
    report(
        &sdk,
        &key,
        json!({"flags":["enabled", "enabled", "disabled", "unassigned", "removed"]}),
        204,
    )
    .await?;
    let usage = admin.get("/api/environments/development/usage").await?;
    let rows = usage["usage"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let timestamp = text(&rows[0]["last_used_at"]);
    assert!(timestamp.ends_with('Z'));
    for row in rows {
        assert_eq!(row["api_key_id"], key["id"]);
        assert_eq!(row["key_name"], "Default");
        assert_eq!(row["application_id"], app["id"]);
        assert_eq!(row["application_name"], "Checkout");
        assert_eq!(row["last_used_at"], timestamp);
        assert!(["enabled", "disabled"].contains(&text(&row["flag_key"])));
    }
    assert!(!usage.to_string().contains(text(&key["secret"])));
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM usage_writes", [], |r| r
            .get::<_, i64>(0))?,
        2
    );
    db.execute_batch("DROP TRIGGER count_usage_insert; DROP TRIGGER count_usage_update; DROP TABLE usage_writes;")?;
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM flag_usage", [], |r| r
            .get::<_, i64>(0))?,
        2
    );
    db.execute("UPDATE flag_usage SET last_used_at = '2000-01-01T00:00:00.000Z' WHERE flag_key = 'disabled'", [])?;
    report(&sdk, &key, json!({"flags":["enabled", "enabled"]}), 204).await?;
    assert_eq!(
        db.query_row(
            "SELECT last_used_at FROM flag_usage WHERE flag_key = 'disabled'",
            [],
            |r| r.get::<_, String>(0)
        )?,
        "2000-01-01T00:00:00.000Z"
    );
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM flag_usage", [], |r| r
            .get::<_, i64>(0))?,
        2
    );
    let last: String = db.query_row(
        "SELECT MAX(last_used_at) FROM flag_usage WHERE api_key_id = ?1",
        [text(&key["id"])],
        |r| r.get(0),
    )?;
    assert_eq!(
        admin.get("/api/environments/development/apps").await?["applications"][0]["keys"][0]
            ["last_used_at"],
        last
    );
    assert_eq!(
        admin.get("/api/environments/development/audits").await?,
        audits
    );
    assert_eq!(
        status(
            sdk.request(Method::GET, "/api/policy")
                .headers(policy_headers(&key))
                .send()
                .await?,
            200
        )
        .await?,
        policy
    );
    assert_eq!(
        sdk.request(Method::GET, "/api/policy")
            .headers(policy_headers(&key))
            .header("If-None-Match", etag)
            .send()
            .await?
            .status(),
        304
    );
    Ok(())
}

#[tokio::test]
async fn usage_is_scoped_and_survives_assignment_changes_and_revocation() -> Result {
    let (server, admin) = fixture().await?;
    let sdk = server.api(false)?;
    for env in ["development", "production"] {
        admin.flag(env, "shared", json!({"mode":"enabled"})).await?;
    }
    let app = admin.app("development", "Checkout", &["shared"]).await?;
    let prod = admin.app("production", "Checkout", &["shared"]).await?;
    let key = app_key(&app, "development");
    let app_path = format!("/api/environments/development/apps/{}", text(&app["id"]));
    let second = admin
        .post(&format!("{app_path}/keys"), json!({"name":"Rotation"}), 200)
        .await?;
    let mut second_key = key.clone();
    second_key["secret"] = second["secret"].clone();
    for scoped_key in [&key, &second_key, &app_key(&prod, "production")] {
        report(&sdk, scoped_key, json!({"flags":["shared"]}), 204).await?;
    }
    let before = admin.get("/api/environments/development/usage").await?;
    assert_eq!(before["usage"].as_array().unwrap().len(), 2);
    assert_eq!(
        admin.get("/api/environments/production/usage").await?["usage"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    for row in before["usage"].as_array().unwrap() {
        assert_eq!(row["application_id"], app["id"]);
        assert_ne!(row["api_key_id"], prod["keys"][0]["id"]);
    }
    admin
        .put(
            &app_path,
            json!({"name":"Checkout", "flags":["shared"]}),
            200,
        )
        .await?;
    assert_eq!(
        admin.get("/api/environments/development/usage").await?,
        before
    );
    admin
        .put(&app_path, json!({"name":"Renamed", "flags":[]}), 200)
        .await?;
    report(&sdk, &key, json!({"flags":["shared"]}), 204).await?;
    let after = admin.get("/api/environments/development/usage").await?;
    for row in after["usage"].as_array().unwrap() {
        let old = before["usage"]
            .as_array()
            .unwrap()
            .iter()
            .find(|old| old["api_key_id"] == row["api_key_id"])
            .unwrap();
        assert_eq!(row["last_used_at"], old["last_used_at"]);
        assert_eq!(row["application_name"], "Renamed");
    }
    admin
        .delete(&format!("/api/keys/{}", text(&key["id"])), 204)
        .await?;
    let response = sdk
        .request(Method::POST, "/api/usage")
        .headers(policy_headers(&key))
        .json(&json!({"flags":["shared"]}))
        .send()
        .await?;
    assert_eq!(response.headers()["x-partly-error"], "key_revoked");
    assert_eq!(status(response, 401).await?["code"], "key_revoked");
    assert_eq!(
        admin.get("/api/environments/development/usage").await?,
        after
    );
    admin
        .delete("/api/environments/development/flags/shared", 204)
        .await?;
    assert_eq!(
        admin.get("/api/environments/development/usage").await?["usage"],
        json!([])
    );
    admin
        .flag("development", "shared", json!({"mode":"enabled"}))
        .await?;
    assert_eq!(
        admin.get("/api/environments/development/usage").await?["usage"],
        json!([])
    );
    assert_eq!(
        admin.get("/api/environments/production/usage").await?["usage"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn usage_validates_reports_and_dashboard_permissions() -> Result {
    let (server, admin) = fixture().await?;
    admin
        .flag("development", "known", json!({"mode":"enabled"}))
        .await?;
    let app = admin.app("development", "Checkout", &["known"]).await?;
    let other = admin.app("development", "Other", &[]).await?;
    let key = app_key(&app, "development");
    let sdk = server.api(false)?;
    for body in [
        json!({"flags":["known", "Bad"]}),
        json!({"flags":[""]}),
        json!({"flags":["a".repeat(65)]}),
        json!({"flags":["known"], "context":{"id":"private"}}),
        json!({"flags":vec!["known"; 129]}),
    ] {
        report(&sdk, &key, body, 400).await?;
    }
    assert_eq!(
        admin.get("/api/environments/development/usage").await?["usage"],
        json!([])
    );
    report(&sdk, &key, json!({"flags":[]}), 204).await?;
    report(&sdk, &key, json!({"flags":vec!["known"; 128]}), 204).await?;
    assert_eq!(
        admin.get("/api/environments/development/usage").await?["usage"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let mut wrong_app = key.clone();
    wrong_app["application_id"] = other["id"].clone();
    for bad in [wrong_app, app_key(&app, "production")] {
        report(&sdk, &bad, json!({"flags":["known"]}), 401).await?;
    }
    for path in ["/api/usage", "/api/environments/development/usage"] {
        let method = if path == "/api/usage" {
            Method::POST
        } else {
            Method::GET
        };
        status(
            sdk.request(method, path)
                .json(&json!({"flags":["known"]}))
                .send()
                .await?,
            401,
        )
        .await?;
    }
    admin
        .user("viewer", json!({"development":"viewer"}), false)
        .await?;
    let viewer = server.login("viewer", PASSWORD).await?;
    assert_eq!(
        viewer.get("/api/environments/development/usage").await?["usage"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    status(
        viewer
            .request(Method::GET, "/api/environments/production/usage")
            .send()
            .await?,
        403,
    )
    .await?;
    status(
        sdk.request(Method::GET, "/api/environments/development/usage")
            .headers(policy_headers(&key))
            .send()
            .await?,
        401,
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn malformed_and_oversized_usage_bodies_do_not_write() -> Result {
    let (server, admin) = fixture().await?;
    admin
        .flag("development", "known", json!({"mode":"enabled"}))
        .await?;
    let app = admin.app("development", "Checkout", &["known"]).await?;
    let key = app_key(&app, "development");
    let sdk = server.api(false)?;
    let audits = admin.get("/api/environments/development/audits").await?;
    for (body, content_type, expected) in [
        ("{".to_string(), Some("application/json"), 400),
        ("[]".to_string(), Some("application/json"), 400),
        (r#"{"flags":{}}"#.to_string(), Some("application/json"), 400),
        (
            r#"{"flags":["known"]}"#.to_string(),
            Some("text/plain"),
            400,
        ),
        (r#"{"flags":["known"]}"#.to_string(), None, 400),
        (
            json!({"flags":["x".repeat(16 * 1024)]}).to_string(),
            Some("application/json"),
            413,
        ),
    ] {
        let mut request = sdk
            .request(Method::POST, "/api/usage")
            .headers(policy_headers(&key))
            .body(body);
        if let Some(content_type) = content_type {
            request = request.header("Content-Type", content_type);
        }
        let response = request.send().await?;
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert!(status(response, expected).await?["error"].is_string());
    }
    assert_eq!(
        admin.get("/api/environments/development/usage").await?["usage"],
        json!([])
    );
    assert_eq!(
        admin.get("/api/environments/development/audits").await?,
        audits
    );
    Ok(())
}

#[tokio::test]
async fn usage_writes_are_atomic_and_foreign_keys_enforce_scope() -> Result {
    let (server, admin) = fixture().await?;
    for env in ["development", "production"] {
        for name in ["alpha", "beta"] {
            admin.flag(env, name, json!({"mode":"enabled"})).await?;
        }
    }
    let app = admin
        .app("development", "Checkout", &["alpha", "beta"])
        .await?;
    let other = admin.app("development", "Other", &["alpha"]).await?;
    let prod = admin.app("production", "Checkout", &["alpha"]).await?;
    let key = app_key(&app, "development");
    let db = server.database()?;
    db.execute_batch("CREATE TRIGGER reject_second_usage BEFORE INSERT ON flag_usage WHEN NEW.flag_key = 'beta' BEGIN SELECT RAISE(ABORT, 'usage failure'); END")?;
    report(
        &server.api(false)?,
        &key,
        json!({"flags":["alpha", "beta"]}),
        500,
    )
    .await?;
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM flag_usage", [], |r| r
            .get::<_, i64>(0))?,
        0
    );
    db.execute_batch("DROP TRIGGER reject_second_usage; PRAGMA foreign_keys = ON;")?;
    for (key_id, app_id, env, flag) in [
        ("missing", text(&app["id"]), "development", "alpha"),
        (text(&key["id"]), text(&other["id"]), "development", "alpha"),
        (text(&key["id"]), text(&app["id"]), "production", "alpha"),
        (text(&key["id"]), text(&prod["id"]), "production", "alpha"),
        (text(&key["id"]), text(&app["id"]), "development", "missing"),
    ] {
        let error = db.execute("INSERT INTO flag_usage (api_key_id, application_id, environment, flag_key, last_used_at) VALUES (?1, ?2, ?3, ?4, '2000-01-01T00:00:00.000Z')", params![key_id, app_id, env, flag]).unwrap_err();
        assert_eq!(
            error.sqlite_error().unwrap().extended_code,
            rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY
        );
    }
    report(
        &server.api(false)?,
        &key,
        json!({"flags":["alpha", "beta"]}),
        204,
    )
    .await?;
    assert_eq!(
        db.query_row("SELECT COUNT(*) FROM flag_usage", [], |r| r
            .get::<_, i64>(0))?,
        2
    );
    Ok(())
}

#[tokio::test]
async fn database_without_usage_table_is_rejected_unchanged() -> Result {
    let (mut server, admin) = fixture().await?;
    admin.app("development", "Checkout", &[]).await?;
    server.stop().await?;
    server.database()?.execute_batch("DROP TABLE flag_usage")?;
    let before = std::fs::read(&server.db)?;
    assert!(server.start().await.is_err());
    assert!(server.log().contains("incompatible development schema"));
    assert_eq!(std::fs::read(&server.db)?, before);
    Ok(())
}
