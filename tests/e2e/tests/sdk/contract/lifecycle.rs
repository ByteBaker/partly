// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;

#[tokio::test]
async fn driver_reports_process_protocol_and_timeout_failures() -> Result {
    for (program, message) in [
        (
            "import sys; sys.stdin.readline(); sys.stderr.write('import failed\\n'); sys.exit(1)",
            "import failed",
        ),
        (
            "import sys; sys.stdin.readline(); print('not json', flush=True)",
            "invalid JSON",
        ),
        (
            "import sys, time; sys.stdin.readline(); print('{', end='', flush=True); time.sleep(10)",
            "timed out",
        ),
        (
            "import sys; sys.stdin.readline(); print('x'*65537, flush=True)",
            "exceeded 64 KiB",
        ),
    ] {
        let mut command = tokio::process::Command::new(python());
        command.args(["-I", "-c", program]);
        let mut driver = Driver::spawn(command, tempfile::tempdir()?)?;
        let timeout = Duration::from_secs(if message == "timed out" { 1 } else { 10 });
        let error = driver
            .call_timeout("init", &mut json!({}), timeout)
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains(message),
            "Expected {message}: {error}"
        );
    }
    Ok(())
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
