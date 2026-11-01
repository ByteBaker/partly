// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;

pub(super) async fn openfeature(language: &str) -> Result {
    let (mut server, admin) = fixture().await?;
    admin
        .flag("development", "off", json!({"mode":"disabled"}))
        .await?;
    admin
        .flag(
            "development",
            "rollout",
            json!({"mode":"percentage","percentage":50}),
        )
        .await?;
    admin
        .flag(
            "development",
            "adult",
            json!({"mode":"custom","action":"enable","groups":[[{"k":"age","op":"ge","v":18}]]}),
        )
        .await?;
    let key = admin.key("development").await?;
    let mut driver = Driver::new(language, true).await?;
    driver.build(&server, &key, 3600.0, true).await?;
    driver.simple("init", json!({"ok":true})).await?;
    if language == "python" {
        driver
            .simple("types_and_context", json!({"ok":true}))
            .await?;
    }
    assert_eq!(
        driver
            .call(
                "evaluate",
                json!({"flag":"off","context":{},"fallback":true})
            )
            .await?,
        json!({"value":false,"error":"","variant":"disabled","reason":"CACHED"})
    );
    assert_eq!(
        driver
            .evaluate("adult", json!({"age":21,"extra":{"unused":true}}))
            .await?["value"],
        true
    );
    for context in [
        json!({}),
        json!({"age":"private"}),
        json!({"age":true}),
        json!({"age":{"nested":21}}),
    ] {
        let result = driver
            .call(
                "evaluate",
                json!({"flag":"adult","context":context,"fallback":true}),
            )
            .await?;
        assert_eq!(result["error"], "INVALID_CONTEXT");
        if language != "rust" {
            assert_eq!(result["value"], true);
        }
    }
    let missing = driver
        .call(
            "evaluate",
            json!({"flag":"missing","context":{},"fallback":true}),
        )
        .await?;
    assert_eq!(missing["error"], "FLAG_NOT_FOUND");
    if language != "rust" {
        assert_eq!(missing["value"], true);
    }
    assert_eq!(
        driver
            .evaluate("rollout", json!({"targetingKey":"user-42","id":"ignored"}))
            .await?,
        driver.evaluate("rollout", json!({"id":"user-42"})).await?
    );
    driver.simple("refresh", json!({"updated":false})).await?;
    admin
        .flag("development", "off", json!({"mode":"enabled"}))
        .await?;
    driver.simple("refresh", json!({"updated":true})).await?;
    assert_eq!(driver.evaluate("off", json!({})).await?["value"], true);
    server.stop().await?;
    assert!(driver.call("refresh", json!({})).await?["error"].is_string());
    assert_eq!(driver.evaluate("off", json!({})).await?["value"], true);
    server.start().await?;
    server
        .login("admin", PASSWORD)
        .await?
        .delete(&format!("/api/keys/{}", text(&key["id"])), 204)
        .await?;
    assert!(driver.call("refresh", json!({})).await?["error"].is_string());
    let revoked = driver.evaluate("off", json!({})).await?;
    assert_eq!(revoked["error"], "PROVIDER_FATAL");
    if language != "rust" {
        assert_eq!(revoked["value"], false);
    }
    driver.simple("close", json!({"ok":true})).await?;
    driver.finish().await
}

#[tokio::test]
async fn python_openfeature_hooks_events_and_provider_isolation() -> Result {
    let (server, admin) = fixture().await?;
    admin
        .flag(
            "development",
            "adult",
            json!({"mode":"custom","action":"enable","groups":[[{"k":"age","op":"ge","v":18}]]}),
        )
        .await?;
    let key = admin.key("development").await?;
    let mut driver = Driver::new("python", true).await?;
    driver.build(&server, &key, 3600.0, true).await?;
    driver.simple("hooks_start", json!({"ok":true})).await?;
    admin
        .flag("development", "adult", json!({"mode":"disabled"}))
        .await?;
    driver.simple("hooks_finish", json!({"ok":true})).await?;
    driver.simple("close", json!({"ok":true})).await?;
    driver.finish().await
}
