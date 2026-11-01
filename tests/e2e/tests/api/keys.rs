// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::support::*;
use reqwest::Method;
use serde_json::json;
use sha2::{Digest, Sha256};

#[tokio::test]
async fn authentication_scopes_and_conditional_policy() -> Result {
    let (server, admin) = fixture().await?;
    let dev = admin.key("development").await?;
    let prod = admin.key("production").await?;
    admin
        .flag(
            "development",
            "new_checkout",
            json!({"mode":"percentage","percentage":30}),
        )
        .await?;
    admin
        .flag("production", "new_checkout", json!({"mode":"disabled"}))
        .await?;
    let sdk = server.api(false)?;
    for path in [
        "/api/environments",
        "/api/environments/development/apps",
        "/api/policy",
    ] {
        assert_eq!(sdk.request(Method::GET, path).send().await?.status(), 401);
    }
    assert_eq!(
        sdk.request(Method::PUT, &format!("/api/keys/{}", text(&dev["id"])))
            .header("X-Partly-Request", "dashboard")
            .json(&json!({"name":"Unauthorized"}))
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        sdk.request(Method::POST, "/api/setup")
            .json(&json!({}))
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        sdk.request(Method::GET, "/api/status")
            .header("Sec-Fetch-Site", "cross-site")
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        sdk.request(Method::GET, "/api/status")
            .header("Host", "rebind.example")
            .send()
            .await?
            .status(),
        421
    );
    assert_eq!(
        admin
            .request(Method::POST, "/api/environments/development/apps")
            .header("Origin", "http://evil.example")
            .send()
            .await?
            .status(),
        403
    );
    assert_eq!(
        admin
            .request(Method::GET, "/api/policy")
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        sdk.request(Method::GET, "/api/environments")
            .headers(policy_headers(&dev))
            .send()
            .await?
            .status(),
        401
    );
    let first = sdk
        .request(Method::GET, "/api/policy")
        .headers(policy_headers(&dev))
        .send()
        .await?;
    let etag = first.headers()["etag"].clone();
    let body = status(first, 200).await?;
    assert_eq!(body["environment"], "development");
    assert_eq!(body["flags"]["new_checkout"]["percentage"], 30);
    for step in 0..3 {
        if step == 1 {
            admin
                .flag(
                    "development",
                    "new_checkout",
                    json!({"mode":"percentage","percentage":30}),
                )
                .await?;
        }
        if step == 2 {
            admin
                .flag(
                    "production",
                    "new_checkout",
                    json!({"mode":"percentage","percentage":60}),
                )
                .await?;
        }
        let response = sdk
            .request(Method::GET, "/api/policy")
            .headers(policy_headers(&dev))
            .header("If-None-Match", &etag)
            .send()
            .await?;
        assert_eq!(response.status(), 304);
        assert!(response.bytes().await?.is_empty());
    }
    admin
        .flag(
            "development",
            "new_checkout",
            json!({"mode":"percentage","percentage":60}),
        )
        .await?;
    let updated = sdk
        .request(Method::GET, "/api/policy")
        .headers(policy_headers(&dev))
        .header("If-None-Match", &etag)
        .send()
        .await?;
    assert_eq!(updated.status(), 200);
    assert_ne!(updated.headers()["etag"], etag);
    assert_eq!(
        status(
            sdk.request(Method::GET, "/api/policy")
                .headers(policy_headers(&prod))
                .send()
                .await?,
            200
        )
        .await?["environment"],
        "production"
    );
    admin
        .delete(&format!("/api/keys/{}", text(&dev["id"])), 204)
        .await?;
    assert_eq!(
        sdk.request(Method::GET, "/api/policy")
            .headers(policy_headers(&dev))
            .header("If-None-Match", etag)
            .send()
            .await?
            .status(),
        401
    );
    admin
        .put(
            "/api/environments/development/flags/INVALID",
            json!({"mode":"percentage","percentage":10}),
            400,
        )
        .await?;
    admin
        .put(
            "/api/environments/development/flags/valid",
            json!({"mode":"percentage","percentage":101}),
            400,
        )
        .await?;
    admin
        .put(
            "/api/environments/development/flags/valid",
            json!({"mode":"percentage","percentage":"10"}),
            422,
        )
        .await?;
    assert_eq!(
        admin
            .request(Method::GET, "/api/environments/unknown/flags")
            .send()
            .await?
            .status(),
        404
    );
    admin
        .delete("/api/environments/development/flags/new_checkout", 204)
        .await?;
    assert_eq!(
        admin.get("/api/environments/development/flags").await?,
        json!({})
    );
    Ok(())
}

#[tokio::test]
async fn named_short_and_long_keys_survive_restart() -> Result {
    let (mut server, admin) = fixture().await?;
    let key = admin.key("development").await?;
    admin
        .flag(
            "development",
            "new_checkout",
            json!({"mode":"percentage","percentage":25}),
        )
        .await?;
    let secret = text(&key["secret"]);
    assert!(
        secret.starts_with("pk_")
            && secret.len() == 27
            && secret[3..].chars().all(|c| c.is_ascii_alphanumeric())
    );
    let legacy = format!("pk_{}", "a1".repeat(32));
    server.stop().await?;
    server.database()?.execute("INSERT INTO sdk_keys (id,application_id,secret,secret_hash,masked_secret,name) VALUES ('legacy-key',?1,?2,?3,?4,'Long key')",
        rusqlite::params![text(&key["application_id"]),legacy,format!("{:x}",Sha256::digest(legacy.as_bytes())),format!("{}••••{}",&legacy[..7],&legacy[legacy.len()-4..])])?;
    server.start().await?;
    let admin = server.login("admin", PASSWORD).await?;
    let records =
        admin.get("/api/environments/development/apps").await?["applications"][0]["keys"].clone();
    assert_eq!(records[0]["name"], "Test application");
    assert_eq!(records[1]["name"], "Long key");
    assert_eq!(
        server
            .database()?
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
        1
    );
    for secret in [secret, legacy.as_str()] {
        let mut check = key.clone();
        check["secret"] = secret.into();
        let policy = status(
            server
                .api(false)?
                .request(Method::GET, "/api/policy")
                .headers(policy_headers(&check))
                .send()
                .await?,
            200,
        )
        .await?;
        assert_eq!(policy["environment"], "development");
        assert_eq!(policy["flags"]["new_checkout"]["percentage"], 25);
        assert!(
            records
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["secret"] == secret)
        );
    }
    admin
        .put(
            &format!("/api/keys/{}", text(&key["id"])),
            json!({"name":"Checkout service"}),
            204,
        )
        .await?;
    server.stop().await?;
    server.start().await?;
    let admin = server.login("admin", PASSWORD).await?;
    assert_eq!(
        admin.get("/api/environments/development/apps").await?["applications"][0]["keys"][0]["name"],
        "Checkout service"
    );
    admin.delete("/api/keys/legacy-key", 204).await?;
    let mut old = key.clone();
    old["secret"] = legacy.into();
    assert_eq!(
        server
            .api(false)?
            .request(Method::GET, "/api/policy")
            .headers(policy_headers(&old))
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        server
            .api(false)?
            .request(Method::GET, "/api/policy")
            .headers(policy_headers(&key))
            .send()
            .await?
            .status(),
        200
    );
    Ok(())
}

#[tokio::test]
async fn key_names_validation_and_environment_isolation() -> Result {
    let (_server, admin) = fixture().await?;
    let key = admin.key("development").await?;
    let path = format!("/api/keys/{}", text(&key["id"]));
    let create = format!(
        "/api/environments/development/apps/{}/keys",
        text(&key["application_id"])
    );
    admin
        .put(&path, json!({"name":"  Checkout service  "}), 204)
        .await?;
    for name in [
        "".to_string(),
        "   ".into(),
        "x".repeat(65),
        "two\nlines".into(),
        "null\0character".into(),
    ] {
        admin.post(&create, json!({"name":name}), 400).await?;
        admin.put(&path, json!({"name":name}), 400).await?;
    }
    admin.post(&create, json!({}), 422).await?;
    admin
        .post(
            &create,
            json!({"name":"Wrong scope","environment":"production"}),
            422,
        )
        .await?;
    admin
        .put(
            &path,
            json!({"name":"Wrong scope","environment":"production"}),
            422,
        )
        .await?;
    admin
        .put("/api/keys/missing", json!({"name":"Checkout"}), 404)
        .await?;
    assert_eq!(
        admin.get("/api/environments/production/apps").await?["applications"],
        json!([])
    );
    let saved = admin.get("/api/environments/development/apps").await?["applications"][0]["keys"]
        [0]
    .clone();
    assert_eq!(saved["name"], "Checkout service");
    assert_eq!(saved["secret"], key["secret"]);
    admin
        .put(&path, json!({"name":"Checkout worker"}), 204)
        .await?;
    let saved = admin.get("/api/environments/development/apps").await?["applications"][0]["keys"]
        [0]
    .clone();
    assert_eq!(saved["name"], "Checkout worker");
    assert_eq!(saved["secret"], key["secret"]);
    Ok(())
}
