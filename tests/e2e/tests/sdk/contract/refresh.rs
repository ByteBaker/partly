// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;

pub(super) async fn scope(language: &str) -> Result {
    let (server, admin) = fixture().await?;
    let mut driver = Driver::new(language, false).await?;
    for flag in ["shared", "private"] {
        admin
            .flag("development", flag, json!({"mode":"enabled"}))
            .await?;
    }
    let app = admin.app("development", "Storefront", &["shared"]).await?;
    let other = admin.app("development", "Billing", &["private"]).await?;
    let key = app_key(&app, "development");
    let mut wrong = key.clone();
    wrong["application_id"] = other["id"].clone();
    driver.build(&server, &wrong, 3600.0, true).await?;
    driver
        .simple("init", json!({"error":"authentication"}))
        .await?;
    driver.simple("close", json!({"ok":true})).await?;
    driver.build(&server, &key, 3600.0, true).await?;
    driver.simple("init", json!({"ok":true})).await?;
    assert_eq!(
        driver.evaluate("shared", json!({})).await?,
        json!({"decision":"enabled"})
    );
    assert_eq!(
        driver.evaluate("private", json!({})).await?,
        json!({"error":"flag_not_found"})
    );
    admin
        .put(
            &format!("/api/environments/development/apps/{}", text(&app["id"])),
            json!({"name":"Storefront","flags":[]}),
            200,
        )
        .await?;
    driver.simple("refresh", json!({"updated":true})).await?;
    assert_eq!(
        driver.evaluate("shared", json!({})).await?,
        json!({"error":"flag_not_found"})
    );
    driver.simple("refresh", json!({"updated":false})).await?;
    driver.simple("close", json!({"ok":true})).await?;
    driver.build(&server, &key, 0.02, true).await?;
    driver.simple("init", json!({"ok":true})).await?;
    admin
        .delete(&format!("/api/keys/{}", text(&key["id"])), 204)
        .await?;
    wait_evaluate(
        &mut driver,
        "shared",
        json!({}),
        json!({"error":"key_revoked"}),
    )
    .await?;
    for op in ["refresh", "init"] {
        driver.simple(op, json!({"error":"key_revoked"})).await?;
    }
    driver.simple("close", json!({"ok":true})).await?;
    assert_eq!(
        driver.evaluate("shared", json!({})).await?,
        json!({"error":"key_revoked"})
    );
    driver.finish().await
}

pub(super) async fn bad_refresh(language: &str) -> Result {
    use crate::support::http::{HttpStub, Reply};
    let policy = json!({"schema_version":1,"application_id":"app_test","project":"default","environment":"development","revision":1,"flags":{"flag":{"mode":"enabled"}}});
    let server = HttpStub::new(Reply::policy(304, vec![], Some("\"one\""))).await?;
    let mut driver = Driver::new(language, false).await?;
    assert_eq!(driver.call("build",json!({"url":server.url,"key":"test-secret","environment":"development","application_id":"app_test","interval":3600,"timeout":1})).await?,json!({"ok":true}));
    driver
        .simple("init", json!({"error":"invalid_policy"}))
        .await?;
    let mut wrong = policy.clone();
    wrong["application_id"] = "app_wrong".into();
    server.respond(Reply::policy(200, wrong.to_string(), Some("\"one\"")));
    driver
        .simple("init", json!({"error":"invalid_policy"}))
        .await?;
    server.respond(Reply::policy(200, policy.to_string(), Some("\"one\"")));
    driver.simple("init", json!({"ok":true})).await?;
    let mut invalid = vec![
        b"not json".to_vec(),
        b"[]".to_vec(),
        vec![b' '; 1024 * 1024 + 1],
    ];
    for (field, value) in [
        ("schema_version", json!(2)),
        ("environment", json!("production")),
        ("application_id", json!("app_other")),
        ("unexpected", json!(true)),
        (
            "flags",
            json!({"flag":{"mode":"percentage","percentage":101}}),
        ),
        (
            "flags",
            json!({"flag":{"mode":"custom","action":"enable","groups":[[{"k":"name","op":"regex","v":"(?i)pro"}]]}}),
        ),
    ] {
        let mut body = policy.clone();
        body[field] = value;
        invalid.push(body.to_string().into_bytes());
    }
    let mut responses: Vec<_> = invalid
        .into_iter()
        .map(|body| (Reply::policy(200, body, Some("\"two\"")), "invalid_policy"))
        .collect();
    responses.extend([
        (
            Reply::policy(200, policy.to_string(), None),
            "invalid_policy",
        ),
        (
            Reply::policy(200, policy.to_string(), Some("W/\"two\"")),
            "invalid_policy",
        ),
        (
            Reply::policy(304, vec![], Some("\"wrong\"")),
            "invalid_policy",
        ),
    ]);
    let mut redirect = Reply::policy(302, vec![], None);
    redirect
        .headers
        .push(("Location".into(), format!("{}/leaked", server.url)));
    responses.push((redirect, "refresh"));
    responses.push((
        Reply::policy(403, "secret response body", None),
        "authentication",
    ));
    let mut slow = Reply::policy(200, policy.to_string(), Some("\"two\""));
    slow.delay = Duration::from_secs(2);
    responses.push((slow, "refresh"));
    for (reply, error) in responses {
        server.respond(reply);
        driver.simple("refresh", json!({"error":error})).await?;
        assert_eq!(
            server.requests.lock().unwrap().last(),
            Some(&("/api/policy".into(), Some("\"one\"".into())))
        );
        assert_eq!(
            driver.evaluate("flag", json!({})).await?,
            json!({"decision":"enabled"})
        );
    }
    let mut changed = policy;
    changed["flags"]["flag"]["mode"] = "disabled".into();
    server.respond(Reply::policy(200, changed.to_string(), Some("\"two\"")));
    driver.simple("refresh", json!({"updated":true})).await?;
    assert_eq!(
        driver.evaluate("flag", json!({})).await?,
        json!({"decision":"disabled"})
    );
    server.respond(Reply::policy(304, vec![], Some("\"two\"")));
    driver.simple("refresh", json!({"updated":false})).await?;
    assert_eq!(
        server.requests.lock().unwrap().last(),
        Some(&("/api/policy".into(), Some("\"two\"".into())))
    );
    assert!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|(path, _)| path == "/api/policy")
    );
    driver.simple("close", json!({"ok":true})).await?;
    driver.finish().await
}
