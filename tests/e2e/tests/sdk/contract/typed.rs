// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;

pub(super) async fn contract(language: &str) -> Result {
    for of in [false, true] {
        let (mut server, admin) = fixture().await?;
        let fixture: Value =
            serde_json::from_slice(&fs::read(root().join("tests/sdk_contract/typed.json"))?)?;
        for name in ["Layout", "Settings"] {
            admin
                .post(
                    "/api/environments/development/types",
                    json!({"name":name,"definition":fixture["types"][name]}),
                    201,
                )
                .await?;
        }
        for (key, flag) in fixture["flags"].as_object().unwrap() {
            admin.flag("development", key, flag.clone()).await?;
        }
        let key = admin.key("development").await?;
        let mut driver = Driver::new(language, of).await?;
        driver.build(&server, &key, 3600.0, true).await?;
        driver.simple("init", json!({"ok":true})).await?;
        for (flag, kind, fallback, context, value, variant, name) in [
            (
                "text",
                "string",
                json!("fallback"),
                json!({}),
                json!("Welcome"),
                "selected",
                Value::Null,
            ),
            (
                "count",
                "integer",
                json!(0),
                json!({}),
                json!(-9007199254740991_i64),
                "other",
                Value::Null,
            ),
            (
                "ratio",
                "float",
                json!(0.5),
                json!({}),
                json!(2.5),
                "selected",
                Value::Null,
            ),
            (
                "layout",
                "string",
                json!("fallback"),
                json!({}),
                json!("wide"),
                "selected",
                json!("Layout"),
            ),
            (
                "settings",
                "object",
                json!({}),
                json!({"age":21}),
                fixture["flags"]["settings"]["selected"].clone(),
                "selected",
                json!("Settings"),
            ),
            (
                "settings",
                "object",
                json!({}),
                json!({"age":17}),
                fixture["flags"]["settings"]["other"].clone(),
                "other",
                json!("Settings"),
            ),
        ] {
            let output = driver
                .call(
                    "resolve",
                    json!({"flag":flag,"kind":kind,"context":context,"fallback":fallback}),
                )
                .await?;
            assert_value(&output["value"], &value);
            assert_eq!(output["variant"], variant);
            assert_eq!(output["named_type"], name);
        }
        for (flag, kind, context, native, standard) in [
            ("text", "boolean", json!({}), "result_type", "TYPE_MISMATCH"),
            (
                "legacy",
                "string",
                json!({}),
                "result_type",
                "TYPE_MISMATCH",
            ),
            (
                "missing",
                "string",
                json!({}),
                "flag_not_found",
                "FLAG_NOT_FOUND",
            ),
            (
                "settings",
                "object",
                json!({}),
                "invalid_context",
                "INVALID_CONTEXT",
            ),
            (
                "settings",
                "object",
                json!({"age":"secret"}),
                "context_type",
                "INVALID_CONTEXT",
            ),
        ] {
            let fallback = match kind {
                "string" => json!("fallback"),
                "object" => json!({"fallback":true}),
                _ => json!(false),
            };
            let output = driver
                .call(
                    "resolve",
                    json!({"flag":flag,"kind":kind,"context":context,"fallback":fallback}),
                )
                .await?;
            assert_eq!(output["error"], if of { standard } else { native });
            if of && language != "rust" {
                assert_eq!(output["value"], fallback);
            }
        }
        if !of {
            for expected in ["Settings", "Layout"] {
                let output = driver.call("resolve", json!({"flag":"settings","kind":"named","expected_type":expected,"context":{"age":21}})).await?;
                if expected == "Settings" {
                    assert_value(&output["value"], &fixture["flags"]["settings"]["selected"]);
                } else {
                    assert_eq!(output["error"], "named_type");
                }
            }
        }
        driver.simple("refresh", json!({"updated":false})).await?;
        let mut changed = fixture["flags"]["text"].clone();
        changed["selected"] = json!("Updated");
        admin.flag("development", "text", changed).await?;
        driver.simple("refresh", json!({"updated":true})).await?;
        server.stop().await?;
        assert!(driver.call("refresh", json!({})).await?["error"].is_string());
        assert_eq!(
            driver
                .call(
                    "resolve",
                    json!({"flag":"text","kind":"string","context":{},"fallback":"fallback"})
                )
                .await?["value"],
            "Updated"
        );
        server.start().await?;
        server
            .login("admin", PASSWORD)
            .await?
            .delete(&format!("/api/keys/{}", text(&key["id"])), 204)
            .await?;
        assert!(driver.call("refresh", json!({})).await?["error"].is_string());
        assert_eq!(
            driver
                .call(
                    "resolve",
                    json!({"flag":"text","kind":"string","context":{},"fallback":"fallback"})
                )
                .await?["error"],
            if of { "PROVIDER_FATAL" } else { "key_revoked" }
        );
        driver.simple("close", json!({"ok":true})).await?;
        driver.finish().await?;
    }
    Ok(())
}

// JSON transports may spell a whole float as `3` (Go/Node) or `3.0` (Python/Rust).
fn assert_value(actual: &Value, expected: &Value) {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => assert_eq!(a.as_f64(), b.as_f64()),
        (Value::Object(a), Value::Object(b)) => {
            assert_eq!(a.keys().collect::<Vec<_>>(), b.keys().collect::<Vec<_>>());
            for (key, value) in b {
                assert_value(&a[key], value);
            }
        }
        _ => assert_eq!(actual, expected),
    }
}
