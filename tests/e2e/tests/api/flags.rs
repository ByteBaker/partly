// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::support::*;
use reqwest::Method;
use serde_json::{Value, json};

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
    assert!(
        text(
            &admin
                .put(&format!("{path}/small"), flag.clone(), 400)
                .await?["error"]
        )
        .contains("1 MiB")
    );
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
