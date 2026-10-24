// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::support::{driver::*, *};
use serde_json::json;
use std::time::{Duration, Instant};

macro_rules! usage_tests {
    ($($language:ident),+) => {$ (
        mod $language {
            use super::*;
            #[tokio::test] async fn native_usage() -> Result { reports(stringify!($language), false).await }
            #[tokio::test] async fn openfeature_usage() -> Result { reports(stringify!($language), true).await }
        }
    )+};
}
usage_tests!(python, rust, go, node);

async fn reports(language: &str, openfeature: bool) -> Result {
    let mut server = Server::new()?;
    let tls = Tls::new("127.0.0.1", false)?;
    if !openfeature {
        tls.configure(&mut server);
    }
    server.start().await?;
    let admin = server.setup().await?;
    let names: Vec<String> = (0..129).map(|n| format!("usage_{n:03}")).collect();
    for (index, name) in names.iter().enumerate() {
        admin
            .post(
                "/api/environments/development/flags",
                json!({"key":name,"mode":if index % 2 == 0 {"enabled"} else {"disabled"}}),
                201,
            )
            .await?;
    }
    let app = admin
        .app(
            "development",
            "Usage client",
            &names.iter().map(String::as_str).collect::<Vec<_>>(),
        )
        .await?;
    let key = app_key(&app, "development");
    let mut driver = Driver::new(language, openfeature).await?;
    driver.build(&server, &key, 3600.0, true).await?;
    driver.simple("init", json!({"ok":true})).await?;
    let usage_path = "/api/environments/development/usage";
    assert_eq!(admin.get(usage_path).await?["usage"], json!([]));
    for _ in 0..140 {
        driver.evaluate(&names[0], json!({})).await?;
    }
    assert_eq!(
        admin.get(usage_path).await?["usage"],
        json!([]),
        "Repeated checks must not fill the distinct-name batch"
    );
    for (index, name) in names[..128].iter().enumerate() {
        let result = driver.evaluate(name, json!({})).await?;
        if openfeature {
            assert_eq!(result["error"], "");
            assert_eq!(result["value"], index % 2 == 0);
        } else {
            assert_eq!(
                result,
                json!({"decision":if index % 2 == 0 {"enabled"} else {"disabled"}})
            );
        }
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    let recorded = loop {
        let rows = admin.get(usage_path).await?["usage"].clone();
        if rows.as_array().unwrap().len() == 128 {
            break rows;
        }
        assert!(
            Instant::now() < deadline,
            "{language}: usage batch did not arrive: {rows}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    assert!(recorded
        .as_array()
        .unwrap()
        .iter()
        .all(|row| row["api_key_id"] == app["keys"][0]["id"]));
    driver.evaluate(&names[128], json!({})).await?;
    driver.simple("close", json!({"ok":true})).await?;
    driver.finish().await?;
    assert_eq!(
        admin.get(usage_path).await?["usage"],
        recorded,
        "Close must not dispatch the idle tail"
    );
    Ok(())
}
