// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::support::*;
use reqwest::Method;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;

#[tokio::test]
async fn delayed_setup_and_restart() -> Result {
    let mut server = Server::new()?;
    server.start().await?;
    let admin = server.api(true)?;
    assert_eq!(admin.request(Method::GET, "/").send().await?.status(), 200);
    assert_eq!(admin.get("/api/status").await?["setup_required"], true);
    admin
        .post(
            "/api/setup",
            json!({"username":"admin","password":"short"}),
            400,
        )
        .await?;
    assert!(!server.db.exists());
    server.stop().await?;
    server.start().await?;
    assert!(!server.db.exists());
    let response = admin
        .request(Method::POST, "/api/setup")
        .json(&json!({"username":"admin","password":PASSWORD}))
        .send()
        .await?;
    let cookie = response.headers()["set-cookie"].to_str()?.to_owned();
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Strict"));
    status(response, 200).await?;
    assert!(server.db.exists());
    admin
        .post(
            "/api/setup",
            json!({"username":"other","password":PASSWORD}),
            409,
        )
        .await?;
    assert_eq!(admin.get("/api/status").await?["setup_required"], false);
    admin
        .flag(
            "development",
            "new_checkout",
            json!({"mode":"percentage","percentage":25}),
        )
        .await?;
    let key = admin.key("development").await?;
    let response = admin
        .request(Method::GET, "/api/environments/development/apps")
        .send()
        .await?;
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(
        status(response, 200).await?["applications"][0]["keys"][0]["secret"],
        key["secret"]
    );
    assert_eq!(
        admin
            .request(
                Method::GET,
                &format!("/api/keys/{}/reveal", text(&key["id"]))
            )
            .send()
            .await?
            .status(),
        404
    );
    let hash: String =
        server
            .database()?
            .query_row("SELECT password_hash FROM users", [], |r| r.get(0))?;
    assert!(hash.starts_with("$argon2id$") && !hash.contains(PASSWORD));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&server.db)?.permissions().mode() & 0o777,
            0o600
        );
    }
    server.stop().await?;
    server.start().await?;
    assert_eq!(
        admin
            .request(Method::GET, "/api/environments")
            .send()
            .await?
            .status(),
        401
    );
    admin
        .post(
            "/api/login",
            json!({"username":"admin","password":"wrong"}),
            401,
        )
        .await?;
    let response = admin
        .request(Method::POST, "/api/login")
        .json(&json!({"username":"admin","password":PASSWORD}))
        .send()
        .await?;
    let cookie = response.headers()["set-cookie"]
        .to_str()?
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    status(response, 200).await?;
    assert_eq!(
        admin.get("/api/environments/development/apps").await?["applications"][0]["keys"][0]
            ["secret"],
        key["secret"]
    );
    assert_eq!(
        admin.get("/api/environments/development/flags").await?["new_checkout"]["percentage"],
        25
    );
    assert!(!server.log().contains(text(&key["secret"])));
    admin.post("/api/logout", json!({}), 204).await?;
    assert_eq!(
        admin
            .request(Method::GET, "/api/environments")
            .send()
            .await?
            .status(),
        401
    );
    assert_eq!(
        admin
            .request(Method::GET, "/api/environments")
            .header("Cookie", cookie)
            .send()
            .await?
            .status(),
        401
    );
    Ok(())
}

#[tokio::test]
async fn concurrent_setup_creates_only_one_admin() -> Result {
    let mut server = Server::new()?;
    server.start().await?;
    let first = server.api(true)?;
    let second = server.api(true)?;
    let (a, b) = tokio::join!(
        first
            .request(Method::POST, "/api/setup")
            .json(&json!({"username":"first","password":PASSWORD}))
            .send(),
        second
            .request(Method::POST, "/api/setup")
            .json(&json!({"username":"second","password":PASSWORD}))
            .send()
    );
    let mut statuses = [a?.status().as_u16(), b?.status().as_u16()];
    statuses.sort();
    assert_eq!(statuses, [200, 409]);
    assert_eq!(
        server
            .database()?
            .query_row("SELECT COUNT(*) FROM users", [], |r| r.get::<_, i64>(0))?,
        1
    );
    Ok(())
}

#[tokio::test]
async fn invalid_existing_database_is_preserved() -> Result {
    for content in [b"not sqlite\0".as_slice(), b""] {
        let mut server = Server::new()?;
        fs::write(&server.db, content)?;
        assert!(server.start().await.is_err());
        assert!(server.log().contains("could not start"));
        assert_eq!(fs::read(&server.db)?, content);
    }
    Ok(())
}

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
async fn empty_policy_is_a_document() -> Result {
    let (server, admin) = fixture().await?;
    let key = admin.key("development").await?;
    let response = server
        .api(false)?
        .request(Method::GET, "/api/policy")
        .headers(policy_headers(&key))
        .send()
        .await?;
    assert!(response.headers().contains_key("etag"));
    assert_eq!(status(response, 200).await?["flags"], json!({}));
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
        assert!(records
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["secret"] == secret));
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
        admin.get("/api/environments/development/apps").await?["applications"][0]["keys"][0]
            ["name"],
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

#[tokio::test]
async fn earlier_development_schema_is_rejected_without_changes() -> Result {
    for sql in ["ALTER TABLE sdk_keys DROP COLUMN name","ALTER TABLE users RENAME TO admin","DROP TABLE permissions","DROP TABLE flags; CREATE TABLE flags (environment TEXT,key TEXT,enabled INTEGER,percentage INTEGER)"] {
        let (mut server,admin)=fixture().await?; admin.key("development").await?; server.stop().await?;
        server.database()?.execute_batch(sql)?; let before=fs::read(&server.db)?;
        assert!(server.start().await.is_err()); assert!(server.log().contains("incompatible development schema"));
        assert_eq!(fs::read(&server.db)?,before);
    }
    Ok(())
}

#[tokio::test]
async fn dashboard_environment_urls() -> Result {
    let mut server = Server::new()?;
    server.start().await?;
    let api = server.api(false)?;
    for env in ["development", "production"] {
        for screen in ["flags", "apps", "audits", "docs", "users"] {
            let response = api
                .request(Method::GET, &format!("/environments/{env}/{screen}"))
                .send()
                .await?;
            assert_eq!(response.status(), 200);
            assert!(response.headers()["content-type"]
                .to_str()?
                .starts_with("text/html"));
        }
    }
    for path in [
        "/environments/unknown/flags",
        "/environments/production/unknown",
    ] {
        assert_eq!(api.request(Method::GET, path).send().await?.status(), 404);
    }
    assert!(!server.db.exists());
    Ok(())
}

#[tokio::test]
async fn flag_creation_updates_and_json_persistence() -> Result {
    let (mut server, admin) = fixture().await?;
    let path = "/api/environments/development/flags";
    let mut configs = json!({"always":{"mode":"enabled"},"never":{"mode":"disabled"},"gradual":{"mode":"percentage","percentage":37},"eligible":{"mode":"custom","action":"enable","groups":[[{"k":"age","op":"ge","v":18},{"k":"plan","op":"matches","v":"pro"}],[{"k":"staff","op":"is","v":true}]]},"excluded":{"mode":"custom","action":"disable","groups":[[{"k":"email","op":"regex","v":"@example\\.com$"}]]}});
    for (key, flag) in configs.as_object().unwrap() {
        admin.flag("development", key, flag.clone()).await?;
    }
    assert_eq!(admin.get(path).await?, configs);
    admin
        .post(path, json!({"key":"gradual","mode":"disabled"}), 409)
        .await?;
    admin
        .put(&format!("{path}/absent"), json!({"mode":"enabled"}), 404)
        .await?;
    admin
        .post(
            "/api/environments/unknown/flags",
            json!({"key":"always","mode":"enabled"}),
            404,
        )
        .await?;
    assert_eq!(
        admin.get("/api/environments/production/flags").await?,
        json!({})
    );
    let key = admin.key("development").await?;
    let sdk = server.api(false)?;
    let first = sdk
        .request(Method::GET, "/api/policy")
        .headers(policy_headers(&key))
        .send()
        .await?;
    let etag = first.headers()["etag"].clone();
    let first = status(first, 200).await?;
    assert!(first["revision"].as_u64().unwrap() >= 1);
    admin
        .flag("development", "eligible", configs["eligible"].clone())
        .await?;
    assert_eq!(
        sdk.request(Method::GET, "/api/policy")
            .headers(policy_headers(&key))
            .header("If-None-Match", etag)
            .send()
            .await?
            .status(),
        304
    );
    configs["gradual"] = json!({"mode":"enabled"});
    admin
        .flag("development", "gradual", configs["gradual"].clone())
        .await?;
    let changed = status(
        sdk.request(Method::GET, "/api/policy")
            .headers(policy_headers(&key))
            .send()
            .await?,
        200,
    )
    .await?;
    assert_eq!(
        changed["revision"].as_u64().unwrap(),
        first["revision"].as_u64().unwrap() + 1
    );
    assert_eq!(changed["flags"], configs);
    {
        let db = server.database()?;
        let mut statement = db.prepare("SELECT key,config FROM flags")?;
        let entries =
            statement.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for entry in entries {
            let (name, config) = entry?;
            assert_eq!(serde_json::from_str::<Value>(&config)?, configs[name]);
        }
    }
    server.stop().await?;
    server.start().await?;
    assert_eq!(
        server.login("admin", PASSWORD).await?.get(path).await?,
        configs
    );
    Ok(())
}

#[tokio::test]
async fn invalid_flags_do_not_change_policy() -> Result {
    let (server, admin) = fixture().await?;
    let path = "/api/environments/development/flags";
    admin
        .flag("development", "existing", json!({"mode":"enabled"}))
        .await?;
    let key = admin.key("development").await?;
    let sdk = server.api(false)?;
    let initial = sdk
        .request(Method::GET, "/api/policy")
        .headers(policy_headers(&key))
        .send()
        .await?;
    let etag = initial.headers()["etag"].clone();
    let mut invalid = vec![
        json!({"mode":"unknown"}),
        json!({"mode":"enabled","percentage":50}),
        json!({"mode":"percentage","percentage":-1}),
        json!({"mode":"percentage","percentage":101}),
        json!({"mode":"custom","action":"enable","groups":[]}),
        json!({"mode":"custom","action":"enable","groups":[[]]}),
        json!({"mode":"custom","action":"other","groups":[[{"k":"age","op":"ge","v":18}]]}),
    ];
    for (op, value) in [
        ("other", json!(18)),
        ("ge", json!("18")),
        ("eq", json!(true)),
        ("eq", json!("18")),
        ("prefix", json!(1)),
        ("matches", json!(false)),
        ("is", json!(1)),
        ("is", json!("true")),
        ("gt", Value::Null),
        ("gt", json!([])),
        ("gt", json!({})),
        ("regex", json!("[")),
        ("regex", json!("(?i)example")),
        ("regex", json!("(?=example)")),
    ] {
        invalid.push(
            json!({"mode":"custom","action":"enable","groups":[[{"k":"value","op":op,"v":value}]]}),
        );
    }
    invalid.push(json!({"mode":"custom","action":"enable","groups":[[{"k":"age","op":"gt","v":18,"extra":true}]]}));
    for flag in invalid {
        let mut body = flag.clone();
        body["key"] = "invalid".into();
        let response = admin.request(Method::POST, path).json(&body).send().await?;
        assert!([400, 422].contains(&response.status().as_u16()));
        let impact = admin.get(&format!("{path}/existing/impact")).await?;
        let response = admin
            .request(Method::PUT, &format!("{path}/existing"))
            .header("X-Partly-Confirm", "development/existing")
            .header("X-Partly-Impact", text(&impact["token"]))
            .json(&flag)
            .send()
            .await?;
        assert!([400, 422].contains(&response.status().as_u16()));
        assert_eq!(
            admin.get(path).await?,
            json!({"existing":{"mode":"enabled"}})
        );
        let response = sdk
            .request(Method::GET, "/api/policy")
            .headers(policy_headers(&key))
            .header("If-None-Match", &etag)
            .send()
            .await?;
        assert_eq!(response.status(), 304);
        assert!(response.bytes().await?.is_empty());
    }
    Ok(())
}

#[tokio::test]
async fn flag_authentication_precedes_rule_parsing() -> Result {
    let (server, admin) = fixture().await?;
    let path = "/api/environments/development/flags";
    let invalid = json!({"key":"invalid","mode":"custom","action":"enable","groups":[[{"k":"email","op":"regex","v":"["}]]});
    let outsider = server.api(true)?;
    outsider.post(path, invalid.clone(), 401).await?;
    outsider
        .put(&format!("{path}/invalid"), invalid.clone(), 401)
        .await?;
    admin.post(path, invalid, 422).await?;
    for (body, content_type, expected) in [
        (
            r#"{"key":"duplicate","mode":"enabled","mode":"disabled"}"#,
            "application/json",
            422,
        ),
        ("{", "application/json", 400),
        (
            r#"{"key":"wrong_type","mode":"enabled"}"#,
            "text/plain",
            415,
        ),
        (
            r#"{"key":"vendor_type","mode":"enabled"}"#,
            "application/vnd.partly+json; charset=utf-8",
            201,
        ),
    ] {
        assert_eq!(
            admin
                .request(Method::POST, path)
                .header("Content-Type", content_type)
                .body(body)
                .send()
                .await?
                .status()
                .as_u16(),
            expected
        );
    }
    Ok(())
}

#[tokio::test]
async fn policy_size_limit_rejects_write_without_changing_snapshot() -> Result {
    let (server, admin) = fixture().await?;
    let path = "/api/environments/development/flags";
    let key = admin.key("development").await?;
    let sdk = server.api(false)?;
    let flag = json!({"mode":"custom","action":"enable","groups":[[{"k":"text","op":"matches","v":"x".repeat(15000)}]]});
    let mut previous = sdk
        .request(Method::GET, "/api/policy")
        .headers(policy_headers(&key))
        .send()
        .await?
        .headers()["etag"]
        .clone();
    let mut rejected = false;
    for i in 0..75 {
        let mut body = flag.clone();
        body["key"] = format!("flag_{i}").into();
        let response = admin.request(Method::POST, path).json(&body).send().await?;
        if response.status() == 400 {
            assert!(text(&response.json::<Value>().await?["error"]).contains("1 MiB"));
            assert_eq!(
                sdk.request(Method::GET, "/api/policy")
                    .headers(policy_headers(&key))
                    .header("If-None-Match", &previous)
                    .send()
                    .await?
                    .status(),
                304
            );
            assert!(admin.get(path).await?.get(format!("flag_{i}")).is_none());
            rejected = true;
            break;
        }
        status(response, 201).await?;
        let policy = sdk
            .request(Method::GET, "/api/policy")
            .headers(policy_headers(&key))
            .send()
            .await?;
        previous = policy.headers()["etag"].clone();
        assert!(policy.bytes().await?.len() <= 1024 * 1024);
    }
    assert!(rejected, "Accepted an oversized policy");
    admin
        .post(path, json!({"key":"small","mode":"enabled"}), 201)
        .await?;
    let before = sdk
        .request(Method::GET, "/api/policy")
        .headers(policy_headers(&key))
        .send()
        .await?
        .headers()["etag"]
        .clone();
    assert!(text(
        &admin
            .put(&format!("{path}/small"), flag.clone(), 400)
            .await?["error"]
    )
    .contains("1 MiB"));
    assert_eq!(
        sdk.request(Method::GET, "/api/policy")
            .headers(policy_headers(&key))
            .header("If-None-Match", before)
            .send()
            .await?
            .status(),
        304
    );
    assert_eq!(admin.get(path).await?["small"], json!({"mode":"enabled"}));
    admin
        .put(&format!("{path}/flag_0"), json!({"mode":"disabled"}), 200)
        .await?;
    let mut body = flag;
    body["key"] = "replacement".into();
    admin.post(path, body, 201).await?;
    Ok(())
}

#[tokio::test]
async fn existing_database_with_invalid_data_is_preserved() -> Result {
    for (kind, error) in [
        ("reference", "invalid references"),
        ("number", "Invalid stored flag configuration"),
        ("oversized", "1 MiB POC limit"),
    ] {
        let (mut server, admin) = fixture().await?;
        admin.key("development").await?;
        admin
            .flag("development", "new_checkout", json!({"mode":"enabled"}))
            .await?;
        server.stop().await?;
        if kind == "reference" {
            server
                .database()?
                .execute("UPDATE applications SET environment='missing'", [])?;
        } else {
            let config = if kind == "number" {
                r#"{"mode":"custom","action":"enable","groups":[[{"k":"number","op":"eq","v":-9223372036854775809}]]}"#.to_string()
            } else {
                json!({"mode":"custom","action":"enable","groups":[[{"k":"text","op":"matches","v":"x".repeat(1024*1024)}]]}).to_string()
            };
            server
                .database()?
                .execute("UPDATE flags SET config=?1", [config])?;
        }
        let before = fs::read(&server.db)?;
        assert!(server.start().await.is_err());
        assert!(server.log().contains(error), "{}", server.log());
        assert_eq!(fs::read(&server.db)?, before);
    }
    Ok(())
}

#[tokio::test]
async fn rejected_requests_keep_security_headers() -> Result {
    let mut server = Server::new()?;
    server.start().await?;
    let api = server.api(false)?;
    for (method, path, header, value, expected) in [
        (Method::GET, "/api/status", "Host", "rebind.example", 421),
        (
            Method::GET,
            "/api/status",
            "Sec-Fetch-Site",
            "cross-site",
            403,
        ),
        (
            Method::POST,
            "/api/setup",
            "Content-Type",
            "application/json",
            403,
        ),
        (
            Method::GET,
            "/api/status",
            "Origin",
            "http://other.example",
            403,
        ),
    ] {
        let response = api
            .request(method, path)
            .header(header, value)
            .body("{}")
            .send()
            .await?;
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert!(response.headers()["content-security-policy"]
            .to_str()?
            .contains("frame-ancestors 'none'"));
        assert!(status(response, expected).await?["error"].is_string());
    }
    Ok(())
}
