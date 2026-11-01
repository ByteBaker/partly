// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;

pub(super) async fn contract(language: &str) -> Result {
    let mut driver = Driver::new(language, false).await?;
    let mut server = Server::new()?;
    let tls = Tls::new("127.0.0.1", false)?;
    tls.configure(&mut server);
    let invalid =
        json!({"secret":"invalid-key","environment":"development","application_id":"app_test"});
    driver.build(&server, &invalid, 3600.0, true).await?;
    assert_eq!(
        driver.evaluate("always", json!({})).await?,
        json!({"error":"policy_not_ready"})
    );
    driver.simple("init", json!({"error":"refresh"})).await?;
    server.start().await?;
    let admin = server.setup().await?;
    driver
        .simple("init", json!({"error":"authentication"}))
        .await?;
    driver.simple("close", json!({"ok":true})).await?;
    let key = admin.key("development").await?;
    for (name, mode) in [("always", "enabled"), ("never", "disabled")] {
        admin
            .flag("development", name, json!({"mode":mode}))
            .await?;
    }
    for (name, percentage) in [("zero", 0), ("checkout", 50), ("new_checkout", 50)] {
        admin
            .flag(
                "development",
                name,
                json!({"mode":"percentage","percentage":percentage}),
            )
            .await?;
    }
    let cases: Value =
        serde_json::from_slice(&fs::read(root().join("tests/sdk_contract/rules.json"))?)?;
    for case in cases.as_array().unwrap() {
        admin.flag("development",&format!("rule_{}",text(&case[0])),json!({"mode":"custom","action":"enable","groups":[[{"k":"value","op":case[0],"v":case[1]}]]})).await?;
    }
    let groups = json!([[{"k":"plan","op":"matches","v":"pro"},{"k":"age","op":"ge","v":18}],[{"k":"beta","op":"is","v":true}]]);
    admin
        .flag(
            "development",
            "groups",
            json!({"mode":"custom","action":"enable","groups":groups}),
        )
        .await?;
    for (name, op, value) in [
        ("large_integer", "eq", json!(9007199254740993_u64)),
        ("unsigned_max", "eq", json!(u64::MAX)),
        ("signed_min", "eq", json!(i64::MIN)),
        ("rounded_float", "eq", json!(1000000000000000100_u64)),
        ("fraction", "gt", json!(1.20000003)),
    ] {
        admin.flag("development",name,json!({"mode":"custom","action":"enable","groups":[[{"k":"value","op":op,"v":value}]]})).await?;
    }
    driver.build(&server, &key, 3600.0, false).await?;
    driver.simple("init", json!({"error":"refresh"})).await?;
    driver.simple("close", json!({"ok":true})).await?;
    driver.build(&server, &key, 3600.0, true).await?;
    driver.simple("init", json!({"ok":true})).await?;
    driver.simple("init", json!({"ok":true})).await?;
    for (flag, decision) in [("always", "enabled"), ("never", "disabled")] {
        assert_eq!(
            driver.evaluate(flag, json!({})).await?,
            json!({"decision":decision})
        );
    }
    assert_eq!(
        driver.evaluate("zero", json!({"id":"user-42"})).await?,
        json!({"decision":"disabled"})
    );
    assert_eq!(
        driver.evaluate("undefined", json!({})).await?,
        json!({"error":"flag_not_found"})
    );
    for (value, actual) in [
        (Value::Null, "null"),
        (json!([]), "array"),
        (json!({"nested":true}), "object"),
    ] {
        assert_eq!(
            driver.evaluate("always", json!({"extra":value})).await?,
            type_error(
                "always",
                "extra",
                Value::Null,
                "string, number, or boolean",
                actual
            )
        );
        assert_eq!(
            driver.evaluate("rule_ge", json!({"value":value})).await?,
            type_error("rule_ge", "value", json!("ge"), "number", actual)
        );
        assert_eq!(
            driver.evaluate("undefined", json!({"value":value})).await?,
            json!({"error":"flag_not_found"})
        );
    }
    assert_eq!(
        driver
            .evaluate("undefined", json!({"value":36893488147419103232.0}))
            .await?,
        json!({"error":"flag_not_found"})
    );
    for (flag, user, decision) in [
        ("checkout", "user-42", "disabled"),
        ("new_checkout", "user-123", "enabled"),
        ("checkout", "用户-1", "disabled"),
    ] {
        assert_eq!(
            driver.evaluate(flag, json!({"id":user})).await?,
            json!({"decision":decision})
        );
    }
    for context in [json!({}), json!({"id":""})] {
        assert_eq!(
            driver.evaluate("checkout", context).await?,
            json!({"error":"invalid_context"})
        );
    }
    assert_eq!(
        driver.evaluate("checkout", json!({"id":42})).await?,
        type_error("checkout", "id", Value::Null, "string", "number")
    );
    let mut selected = vec![];
    for user in 0..10 {
        selected.push(
            driver
                .evaluate("checkout", json!({"id":format!("user-{user}")}))
                .await?,
        );
    }
    let mut expanded = vec![];
    for user in 0..100 {
        expanded.push(
            driver
                .evaluate("checkout", json!({"id":format!("user-{user}")}))
                .await?,
        );
    }
    assert_eq!(selected, expanded[..10]);
    assert!(
        expanded.contains(&json!({"decision":"enabled"}))
            && expanded.contains(&json!({"decision":"disabled"}))
    );
    for case in cases.as_array().unwrap() {
        let op = text(&case[0]);
        let flag = format!("rule_{op}");
        assert_eq!(
            driver.evaluate(&flag, json!({"value":case[2]})).await?,
            json!({"decision":"enabled"})
        );
        assert_eq!(
            driver.evaluate(&flag, json!({"value":case[3]})).await?,
            json!({"decision":"disabled"})
        );
        assert_eq!(
            driver.evaluate(&flag, json!({"value":case[4]})).await?,
            type_error(&flag, "value", json!(op), text(&case[5]), text(&case[6]))
        );
        assert_eq!(
            driver.evaluate(&flag, json!({})).await?,
            json!({"error":"invalid_context"})
        );
    }
    assert_eq!(
        driver
            .evaluate("rule_matches", json!({"value":"Pro"}))
            .await?,
        json!({"decision":"disabled"})
    );
    for (flag, value, decision) in [
        ("large_integer", json!(9007199254740993_u64), "enabled"),
        ("large_integer", json!(9007199254740992_u64), "disabled"),
        ("unsigned_max", json!(u64::MAX), "enabled"),
        ("unsigned_max", json!(u64::MAX - 1), "disabled"),
        ("signed_min", json!(i64::MIN), "enabled"),
        ("signed_min", json!(i64::MIN + 1), "disabled"),
        (
            "rounded_float",
            json!(1000000000000000100_u64 as f64),
            "disabled",
        ),
        ("rule_eq", json!(1e20), "disabled"),
        ("fraction", json!(1.2000000476837158), "enabled"),
        ("fraction", json!(1.2), "disabled"),
    ] {
        let expected = if language == "node" && value.as_f64().unwrap().abs() > 9007199254740991.0 {
            json!({"error":"invalid_context"})
        } else {
            json!({"decision":decision})
        };
        assert_eq!(
            driver.evaluate(flag, json!({"value":value})).await?,
            expected,
            "{language}: {flag}"
        );
    }
    let member = json!({"plan":"pro","age":21,"beta":false});
    let outsider = json!({"plan":"free","age":16,"beta":false});
    assert_eq!(
        driver.evaluate("groups", member.clone()).await?,
        json!({"decision":"enabled"})
    );
    assert_eq!(
        driver.evaluate("groups", outsider.clone()).await?,
        json!({"decision":"disabled"})
    );
    assert_eq!(
        driver
            .evaluate("groups", json!({"plan":"free","age":16,"beta":true}))
            .await?,
        json!({"decision":"enabled"})
    );
    assert_eq!(
        driver.evaluate("groups", json!({"beta":true})).await?,
        json!({"error":"invalid_context"})
    );
    assert_eq!(
        driver
            .evaluate("groups", json!({"plan":"pro","age":"private-context-value","beta":true,"email":"sam@company.com"}))
            .await?,
        type_error("groups", "age", json!("ge"), "number", "string")
    );
    driver.simple("refresh", json!({"updated":false})).await?;
    admin
        .flag("production", "always", json!({"mode":"disabled"}))
        .await?;
    driver.simple("refresh", json!({"updated":false})).await?;
    admin
        .flag("development", "always", json!({"mode":"disabled"}))
        .await?;
    admin
        .flag(
            "development",
            "checkout",
            json!({"mode":"percentage","percentage":100}),
        )
        .await?;
    admin
        .flag(
            "development",
            "groups",
            json!({"mode":"custom","action":"disable","groups":groups}),
        )
        .await?;
    admin.flag("development","rule_is",json!({"mode":"custom","action":"enable","groups":[[{"k":"value","op":"is","v":false}]]})).await?;
    assert_eq!(
        driver.evaluate("always", json!({})).await?,
        json!({"decision":"enabled"})
    );
    driver.simple("refresh", json!({"updated":true})).await?;
    assert_eq!(
        driver.evaluate("always", json!({})).await?,
        json!({"decision":"disabled"})
    );
    assert_eq!(
        driver.evaluate("groups", member.clone()).await?,
        json!({"decision":"disabled"})
    );
    assert_eq!(
        driver.evaluate("groups", outsider).await?,
        json!({"decision":"enabled"})
    );
    for value in [false, true] {
        assert_eq!(
            driver.evaluate("rule_is", json!({"value":value})).await?,
            json!({"decision":if value {"disabled"}else{"enabled"}})
        );
    }
    for user in 0..100 {
        assert_eq!(
            driver
                .evaluate("checkout", json!({"id":format!("user-{user}")}))
                .await?,
            json!({"decision":"enabled"})
        );
    }
    driver.simple("refresh", json!({"updated":false})).await?;
    driver.simple("close", json!({"ok":true})).await?;
    driver.build(&server, &key, 0.05, true).await?;
    driver.simple("init", json!({"ok":true})).await?;
    admin
        .flag("development", "always", json!({"mode":"enabled"}))
        .await?;
    wait_evaluate(
        &mut driver,
        "always",
        json!({}),
        json!({"decision":"enabled"}),
    )
    .await?;
    admin
        .delete(&format!("/api/keys/{}", text(&key["id"])), 204)
        .await?;
    for op in ["refresh", "init"] {
        driver.simple(op, json!({"error":"key_revoked"})).await?;
    }
    assert_eq!(
        driver.evaluate("always", json!({})).await?,
        json!({"error":"key_revoked"})
    );
    driver.simple("close", json!({"ok":true})).await?;
    assert_eq!(
        driver.evaluate("always", json!({})).await?,
        json!({"error":"key_revoked"})
    );
    let key = admin.key("development").await?;
    driver.build(&server, &key, 3600.0, true).await?;
    driver.simple("init", json!({"ok":true})).await?;
    server.stop().await?;
    driver.simple("refresh", json!({"error":"refresh"})).await?;
    assert_eq!(
        driver.evaluate("always", json!({})).await?,
        json!({"decision":"enabled"})
    );
    assert_eq!(
        driver.evaluate("groups", member).await?,
        json!({"decision":"disabled"})
    );
    driver.simple("close", json!({"ok":true})).await?;
    driver.simple("close", json!({"ok":true})).await?;
    assert_eq!(
        driver.evaluate("always", json!({})).await?,
        json!({"decision":"enabled"})
    );
    for op in ["init", "refresh"] {
        driver.simple(op, json!({"error":"client_closed"})).await?;
    }
    driver.build(&server, &key, 3600.0, true).await?;
    driver.simple("init", json!({"error":"refresh"})).await?;
    assert_eq!(
        driver.evaluate("always", json!({})).await?,
        json!({"error":"policy_not_ready"})
    );
    driver.simple("close", json!({"ok":true})).await?;
    driver.finish().await?;
    assert!(
        !fs::read_to_string(&driver.log_path)?.contains(text(&key["secret"]))
            && !server.log().contains(text(&key["secret"]))
    );
    Ok(())
}
