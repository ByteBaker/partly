// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::support::{driver::*, *};
use serde_json::{json, Value};
use std::{
    fs,
    time::{Duration, Instant},
};

macro_rules! language_tests {
    ($($language:ident),+) => {$ (
        mod $language {
            use super::*;
            #[tokio::test] async fn native_contract() -> Result { contract(stringify!($language)).await }
            #[tokio::test] async fn application_scope_and_background_revocation() -> Result { scope(stringify!($language)).await }
            #[tokio::test] async fn bad_refresh_preserves_snapshot_and_etag() -> Result { bad_refresh(stringify!($language)).await }
            #[tokio::test] async fn openfeature_contract() -> Result { openfeature(stringify!($language)).await }
        }
    )+};
}
language_tests!(python, rust, go, node);

fn type_error(flag: &str, key: &str, op: Value, expected: &str, actual: &str) -> Value {
    json!({"error":"context_type","flag_key":flag,"k":key,"op":op,"expected_type":expected,"actual_type":actual})
}

async fn contract(language: &str) -> Result {
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

pub async fn wait_evaluate(
    driver: &mut Driver,
    flag: &str,
    context: Value,
    expected: Value,
) -> Result {
    let deadline = Instant::now() + Duration::from_secs(5);
    while driver.evaluate(flag, context.clone()).await? != expected {
        assert!(
            Instant::now() < deadline,
            "Background refresh did not reach {expected}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Ok(())
}

async fn scope(language: &str) -> Result {
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

async fn openfeature(language: &str) -> Result {
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

async fn bad_refresh(language: &str) -> Result {
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
    assert!(server
        .requests
        .lock()
        .unwrap()
        .iter()
        .all(|(path, _)| path == "/api/policy"));
    driver.simple("close", json!({"ok":true})).await?;
    driver.finish().await
}

#[tokio::test]
async fn driver_reports_process_protocol_and_timeout_failures() -> Result {
    for (program, message) in [
        ("import sys; sys.stdin.readline(); sys.stderr.write('import failed\\n'); sys.exit(1)","import failed"),
        ("import sys; sys.stdin.readline(); print('not json', flush=True)","invalid JSON"),
        ("import sys, time; sys.stdin.readline(); print('{', end='', flush=True); time.sleep(10)","timed out"),
        ("import sys; sys.stdin.readline(); print('x'*65537, flush=True)","exceeded 64 KiB"),
    ] {
        let mut command = tokio::process::Command::new(python());
        command.args(["-I", "-c", program]);
        let mut driver = Driver::spawn(command, tempfile::tempdir()?)?;
        let timeout = Duration::from_secs(if message == "timed out" { 1 } else { 10 });
        let error = driver.call_timeout("init", &mut json!({}), timeout).await.unwrap_err();
        assert!(error.to_string().contains(message), "Expected {message}: {error}");
    }
    Ok(())
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

#[tokio::test]
async fn python_population_polling_restart_and_cleanup() -> Result {
    let (mut server, admin) = fixture().await?;
    let key = admin.key("development").await?;
    let mut driver = Driver::new("python", false).await?;
    // An empty policy is valid, including on a cold SDK.
    driver.build(&server, &key, 3600.0, true).await?;
    driver.simple("init", json!({"ok":true})).await?;
    driver.simple("refresh", json!({"updated":false})).await?;
    assert_eq!(
        driver.evaluate("none", json!({"id":"u"})).await?,
        json!({"error":"flag_not_found"})
    );
    admin
        .flag(
            "development",
            "new_checkout",
            json!({"mode":"percentage","percentage":25}),
        )
        .await?;
    driver.simple("refresh", json!({"updated":true})).await?;
    let selected = driver
        .call("population", json!({"flag":"new_checkout","count":1000}))
        .await?;
    let selected = selected["selected"].as_array().unwrap().clone();
    assert!(selected.len() > 180 && selected.len() < 320);
    assert_eq!(
        driver
            .call("population", json!({"flag":"new_checkout","count":1000}))
            .await?["selected"],
        json!(selected)
    );
    admin
        .flag(
            "development",
            "new_checkout",
            json!({"mode":"percentage","percentage":50}),
        )
        .await?;
    driver.simple("refresh", json!({"updated":true})).await?;
    let expanded = driver
        .call("population", json!({"flag":"new_checkout","count":1000}))
        .await?;
    let expanded = expanded["selected"].as_array().unwrap();
    assert!(expanded.len() > selected.len() && selected.iter().all(|user| expanded.contains(user)));
    for (flag, count) in [
        (json!({"mode":"percentage","percentage":0}), 0),
        (json!({"mode":"percentage","percentage":100}), 1000),
        (json!({"mode":"disabled"}), 0),
    ] {
        admin.flag("development", "new_checkout", flag).await?;
        driver.simple("refresh", json!({"updated":true})).await?;
        assert_eq!(
            driver
                .call("population", json!({"flag":"new_checkout","count":1000}))
                .await?["selected"]
                .as_array()
                .unwrap()
                .len(),
            count
        );
    }
    admin
        .flag("development", "new_checkout", json!({"mode":"enabled"}))
        .await?;
    driver.simple("refresh", json!({"updated":true})).await?;
    driver.simple("close", json!({"ok":true})).await?;
    driver.build(&server, &key, 0.05, true).await?;
    driver.simple("init", json!({"ok":true})).await?;
    admin
        .flag("development", "new_checkout", json!({"mode":"disabled"}))
        .await?;
    wait_evaluate(
        &mut driver,
        "new_checkout",
        json!({"id":"u"}),
        json!({"decision":"disabled"}),
    )
    .await?;
    admin
        .flag(
            "development",
            "new_checkout",
            json!({"mode":"percentage","percentage":50}),
        )
        .await?;
    let mut command = tokio::process::Command::new(python());
    command
        .arg(root().join("examples/python/rollout.py"))
        .env("PARTLY_URL", &server.url)
        .env("PARTLY_SDK_KEY", text(&key["secret"]))
        .env("PARTLY_ENVIRONMENT", "development")
        .env("PARTLY_APPLICATION_ID", text(&key["application_id"]));
    let result = checked(command, 30).await?;
    driver.call("refresh", json!({})).await?;
    let selected = driver
        .call(
            "population",
            json!({"flag":"new_checkout","start":1,"count":10001}),
        )
        .await?;
    let count = selected["selected"].as_array().unwrap().len();
    assert!((4800..=5200).contains(&count));
    assert_eq!(
        String::from_utf8_lossy(&result.stdout)
            .lines()
            .collect::<Vec<_>>(),
        [
            format!(
                "Enabled for {},{:03} of 10,000 users ({:.1}%).",
                count / 1000,
                count % 1000,
                count as f64 / 100.0
            ),
            "Policy updated: False".into()
        ]
    );
    driver.simple("checkpoint", json!({"ok":true})).await?;
    let before = driver.call("state", json!({})).await?;
    let previous = driver.evaluate("new_checkout", json!({"id":"u"})).await?;
    server.stop().await?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while driver.call("state", json!({})).await?["refresh_failed"] != true {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    driver
        .simple("assert_preserved", json!({"ok":true}))
        .await?;
    assert_eq!(
        driver.evaluate("new_checkout", json!({"id":"u"})).await?,
        previous
    );
    server.start().await?;
    let current = server.login("admin", PASSWORD).await?;
    let mode = if previous["decision"] == "enabled" {
        "disabled"
    } else {
        "enabled"
    };
    current
        .flag("development", "new_checkout", json!({"mode":mode}))
        .await?;
    wait_evaluate(
        &mut driver,
        "new_checkout",
        json!({"id":"u"}),
        json!({"decision":mode}),
    )
    .await?;
    let state = driver.call("state", json!({})).await?;
    assert_eq!(state["refresh_failed"], false);
    assert_ne!(state["etag"], before["etag"]);
    current
        .delete(&format!("/api/keys/{}", text(&key["id"])), 204)
        .await?;
    driver
        .simple("refresh", json!({"error":"key_revoked"}))
        .await?;
    driver.simple("assert_cleared", json!({"ok":true})).await?;
    driver.simple("close", json!({"ok":true})).await?;
    let state = driver.call("state", json!({})).await?;
    assert_eq!(state["poll_done"], true);
    assert_eq!(state["http_closed"], true);
    driver.finish().await
}

#[tokio::test]
async fn saved_policy_precision_environment_and_restart_consistency() -> Result {
    let (mut server, admin) = fixture().await?;
    admin
        .flag(
            "development",
            "checkout",
            json!({"mode":"percentage","percentage":50}),
        )
        .await?;
    admin
        .flag("production", "checkout", json!({"mode":"disabled"}))
        .await?;
    admin
        .flag("development", "public", json!({"mode":"enabled"}))
        .await?;
    for (key, value) in [("minimum", json!(i64::MIN)), ("maximum", json!(u64::MAX))] {
        admin.flag("development",key,json!({"mode":"custom","action":"enable","groups":[[{"k":"number","op":"eq","v":value}]]})).await?;
    }
    let key = admin.key("development").await?;
    let prod_key = admin.key("production").await?;
    let mut dev = Driver::new("python", false).await?;
    dev.build(&server, &key, 3600.0, true).await?;
    dev.simple("init", json!({"ok":true})).await?;
    let mut prod = Driver::new("python", false).await?;
    prod.build(&server, &prod_key, 3600.0, true).await?;
    prod.simple("init", json!({"ok":true})).await?;
    assert_eq!(
        dev.evaluate("public", json!({})).await?,
        json!({"decision":"enabled"})
    );
    assert_eq!(
        prod.evaluate("checkout", json!({})).await?,
        json!({"decision":"disabled"})
    );
    let mut selected = vec![];
    for user in 0..10 {
        selected.push(
            dev.evaluate("checkout", json!({"id":format!("user-{user}")}))
                .await?,
        );
    }
    for (flag, value, neighbor, invalid) in [
        (
            "minimum",
            json!(i64::MIN),
            json!(i64::MIN + 1),
            "-9223372036854775809",
        ),
        (
            "maximum",
            json!(u64::MAX),
            json!(u64::MAX - 1),
            "18446744073709551616",
        ),
    ] {
        assert_eq!(
            dev.evaluate(flag, json!({"number":value})).await?,
            json!({"decision":"enabled"})
        );
        assert_eq!(
            dev.evaluate(flag, json!({"number":neighbor})).await?,
            json!({"decision":"disabled"})
        );
        let path = format!("/api/environments/development/flags/{flag}");
        let impact = admin.get(&format!("{path}/impact")).await?;
        let body = format!(
            r#"{{"mode":"custom","action":"enable","groups":[[{{"k":"number","op":"eq","v":{invalid}}}]]}}"#
        );
        status(
            admin
                .request(reqwest::Method::PUT, &path)
                .header("Content-Type", "application/json")
                .header("X-Partly-Confirm", format!("development/{flag}"))
                .header("X-Partly-Impact", text(&impact["token"]))
                .body(body)
                .send()
                .await?,
            422,
        )
        .await?;
    }
    dev.simple("refresh", json!({"updated":false})).await?;
    assert_eq!(
        dev.evaluate("minimum", json!({"number":i64::MIN})).await?,
        json!({"decision":"enabled"})
    );
    server.stop().await?;
    server.start().await?;
    let mut cold = Driver::new("python", false).await?;
    cold.build(&server, &key, 3600.0, true).await?;
    cold.simple("init", json!({"ok":true})).await?;
    for (user, expected) in selected.into_iter().enumerate() {
        assert_eq!(
            cold.evaluate("checkout", json!({"id":format!("user-{user}")}))
                .await?,
            expected
        );
    }
    cold.simple("refresh", json!({"updated":false})).await?;
    for driver in [&mut dev, &mut prod, &mut cold] {
        driver.simple("close", json!({"ok":true})).await?;
        driver.finish().await?;
    }
    Ok(())
}
