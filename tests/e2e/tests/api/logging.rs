// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::support::*;
use reqwest::Method;
use serde_json::json;
use tokio::process::Command;

fn request_logged(log: &str, level: &str, event: &str, method: &str, route: &str, status: u16) {
    assert!(
        log.lines().any(|line| {
            [level, event, "method=", method, route, "elapsed_ms="]
                .iter()
                .all(|value| line.contains(value))
                && line.contains(&format!("status={status}"))
        }),
        "Missing {level} {event}: {method} {route} {status}\n{log}"
    );
}

#[tokio::test]
async fn default_logs_show_admin_activity_without_healthy_request_noise() -> Result {
    let mut server = Server::new()?;
    server.start().await?;
    server.setup().await?;
    let admin = server.login("admin", PASSWORD).await?;
    admin
        .flag("development", "checkout", json!({"mode":"enabled"}))
        .await?;
    let app = admin.app("development", "Checkout", &["checkout"]).await?;
    let app_path = format!("/api/environments/development/apps/{}", text(&app["id"]));
    admin
        .put(
            &app_path,
            json!({"name":"Checkout service","flags":["checkout"]}),
            200,
        )
        .await?;
    let rotation = admin
        .post(&format!("{app_path}/keys"), json!({"name":"Rotation"}), 200)
        .await?;
    let key_path = format!("/api/keys/{}", text(&rotation["id"]));
    admin.put(&key_path, json!({"name":"Retired"}), 204).await?;
    admin.delete(&key_path, 204).await?;
    let user = admin
        .user("viewer", json!({"development":"viewer"}), false)
        .await?;
    admin
        .update_user(&user, json!({"enabled":false}), 200)
        .await?;
    admin
        .put(
            "/api/environments/development/flags/checkout",
            json!({"mode":"disabled"}),
            200,
        )
        .await?;

    let before_reads = server.log();
    for path in [
        "/",
        "/app.js",
        "/style.css",
        "/logo.svg",
        "/docs/python.py",
        "/api/status",
        "/api/environments",
        "/api/environments/development/flags",
        "/api/environments/development/apps",
        "/api/environments/development/audits",
        "/api/environments/development/usage",
    ] {
        status(admin.request(Method::GET, path).send().await?, 200).await?;
    }
    let sdk = server.api(false)?;
    let key = app_key(&app, "development");
    let response = sdk
        .request(Method::GET, "/api/policy")
        .headers(policy_headers(&key))
        .send()
        .await?;
    let etag = response.headers()["etag"].clone();
    status(response, 200).await?;
    status(
        sdk.request(Method::GET, "/api/policy")
            .headers(policy_headers(&key))
            .header("If-None-Match", etag)
            .send()
            .await?,
        304,
    )
    .await?;
    status(
        sdk.request(Method::POST, "/api/usage")
            .headers(policy_headers(&key))
            .json(&json!({"flags":["checkout"]}))
            .send()
            .await?,
        204,
    )
    .await?;
    assert_eq!(
        server.log(),
        before_reads,
        "Healthy reads and SDK reports must stay quiet at info"
    );

    admin
        .delete("/api/environments/development/flags/checkout", 204)
        .await?;
    admin.post("/api/logout", json!({}), 204).await?;
    server.stop().await?;
    let log = server.log();
    for event in ["Server ready", "Shutdown requested", "Server stopped"] {
        assert!(
            log.lines()
                .any(|line| line.contains("INFO") && line.contains(event)),
            "{log}"
        );
    }
    assert!(log.contains("setup_required=true"), "{log}");
    for (method, route, status) in [
        ("POST", "/api/setup", 200),
        ("POST", "/api/login", 200),
        ("POST", "/api/logout", 204),
        ("POST", "/api/environments/{env}/flags", 201),
        ("PUT", "/api/environments/{env}/flags/{key}", 200),
        ("DELETE", "/api/environments/{env}/flags/{key}", 204),
        ("POST", "/api/environments/{env}/apps", 201),
        ("PUT", "/api/environments/{env}/apps/{id}", 200),
        ("POST", "/api/environments/{env}/apps/{id}/keys", 200),
        ("PUT", "/api/keys/{id}", 204),
        ("DELETE", "/api/keys/{id}", 204),
        ("POST", "/api/users", 201),
        ("PUT", "/api/users/{id}", 200),
    ] {
        request_logged(&log, "INFO", "Request completed", method, route, status);
    }
    assert!(!log.contains("DEBUG") && !log.contains(PASSWORD));
    assert!(!log.contains(text(&key["secret"])) && !log.contains(text(&rotation["secret"])));
    Ok(())
}

#[tokio::test]
async fn debug_and_trace_logs_redact_request_values_and_storage_errors() -> Result {
    for level in ["debug", "trace"] {
        check_redaction(level).await?;
    }
    Ok(())
}

async fn check_redaction(level: &str) -> Result {
    let mut server = Server::new()?;
    server.extra = vec!["--log-level".into(), level.into()];
    server.start().await?;
    let admin = server.api(true)?;
    let response = admin
        .request(Method::POST, "/api/setup")
        .json(&json!({"username":"private-username","password":"private-password-sentinel"}))
        .send()
        .await?;
    let cookie = response.headers()["set-cookie"]
        .to_str()?
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let session_token = cookie.split_once('=').unwrap().1;
    status(response, 200).await?;
    admin
        .flag(
            "development",
            "private-flag-name",
            json!({
                "mode":"custom","action":"enable",
                "groups":[[{"k":"private-context-key","op":"matches","v":"private-rule-value"}]]
            }),
        )
        .await?;
    let app = admin
        .app("development", "private-app-name", &["private-flag-name"])
        .await?;
    let key = app_key(&app, "development");
    let sdk = server.api(false)?;
    status(
        sdk.request(Method::GET, "/api/policy?token=private-query-value")
            .headers(policy_headers(&key))
            .header("X-Forwarded-For", "private-forwarded-address")
            .send()
            .await?,
        200,
    )
    .await?;
    status(
        admin
            .request(Method::from_bytes(b"private-custom-method")?, "/api/status")
            .send()
            .await?,
        405,
    )
    .await?;
    status(
        admin
            .request(
                Method::GET,
                "/api/environments/development/flags/private-path-value/impact",
            )
            .send()
            .await?,
        404,
    )
    .await?;
    status(
        admin
            .request(Method::GET, "/private-unmatched-path")
            .send()
            .await?,
        404,
    )
    .await?;
    status(
        admin
            .request(Method::GET, "/api/status")
            .header("Host", "private-host.invalid")
            .send()
            .await?,
        421,
    )
    .await?;
    status(
        admin
            .request(Method::GET, "/api/status")
            .header("Origin", "https://private-origin.invalid")
            .send()
            .await?,
        403,
    )
    .await?;
    status(
        sdk.request(Method::GET, "/api/policy")
            .bearer_auth("private-authorization")
            .header("Cookie", "partly_session=private-cookie")
            .send()
            .await?,
        401,
    )
    .await?;
    admin
        .post(
            "/api/login",
            json!({"username":"private-login-user","password":"private-login-password"}),
            401,
        )
        .await?;
    admin
        .post(
            "/api/environments/development/flags",
            json!({"key":"private-invalid-flag","mode":"private-invalid-mode"}),
            422,
        )
        .await?;

    server.database()?.execute_batch("CREATE TRIGGER reject_logged_write BEFORE INSERT ON flags BEGIN SELECT RAISE(ABORT, 'private-storage-detail'); END;")?;
    let failed = admin
        .post(
            "/api/environments/development/flags",
            json!({"key":"private-failed-flag","mode":"enabled"}),
            500,
        )
        .await?;
    assert!(!failed.to_string().contains("private-storage-detail"));
    server.stop().await?;
    let log = server.log();
    request_logged(
        &log,
        "DEBUG",
        "Request completed",
        "GET",
        "/api/policy",
        200,
    );
    request_logged(
        &log,
        "DEBUG",
        "Request completed",
        "GET",
        "<unmatched>",
        404,
    );
    request_logged(&log, "WARN", "Request rejected", "GET", "/api/status", 403);
    request_logged(
        &log,
        "DEBUG",
        "Request completed",
        "OTHER",
        "/api/status",
        405,
    );
    request_logged(&log, "WARN", "Sign-in rejected", "POST", "/api/login", 401);
    request_logged(
        &log,
        "ERROR",
        "Request failed",
        "POST",
        "/api/environments/{env}/flags",
        500,
    );
    assert_eq!(
        log.lines()
            .filter(|line| line.contains("ERROR") && line.contains("status=500"))
            .count(),
        1,
        "{log}"
    );
    assert!(
        !log.contains("private-"),
        "Request or storage values leaked: {log}"
    );
    assert!(
        !log.contains(text(&key["secret"])) && !log.contains(session_token),
        "Credentials leaked: {log}"
    );
    Ok(())
}

#[tokio::test]
async fn warning_filter_keeps_authentication_failures_and_limits_throttle_noise() -> Result {
    let mut server = Server::new()?;
    server.extra = vec!["--log-level".into(), "warn".into()];
    server.start().await?;
    let admin = server.setup().await?;
    for attempt in 0..8 {
        admin
            .post(
                "/api/login",
                json!({"username":"admin","password":"private-wrong-password"}),
                if attempt < 5 { 401 } else { 429 },
            )
            .await?;
    }
    server.stop().await?;
    let log = server.log();
    assert_eq!(
        log.lines()
            .filter(|line| line.contains("WARN") && line.contains("Sign-in rejected"))
            .count(),
        5,
        "{log}"
    );
    assert_eq!(
        log.lines()
            .filter(|line| line.contains("WARN") && line.contains("Sign-in requests throttled"))
            .count(),
        1,
        "{log}"
    );
    assert!(
        !log.contains("INFO") && !log.contains("DEBUG") && !log.contains("private-wrong-password"),
        "{log}"
    );
    Ok(())
}

#[tokio::test]
async fn log_level_validation_and_off_preserve_readiness_output() -> Result {
    let mut server = Server::new()?;
    let mut command = Command::new(binary());
    command
        .args(["serve", "--log-level", "invalid", "--db"])
        .arg(&server.db);
    let output = driver::output(command, 10).await?;
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("invalid"));
    assert!(!server.db.exists());

    server.extra = vec!["--log-level".into(), "off".into()];
    server.start().await?;
    let admin = server.setup().await?;
    admin
        .post(
            "/api/login",
            json!({"username":"admin","password":"incorrect"}),
            401,
        )
        .await?;
    server.stop().await?;
    assert_eq!(
        server.log(),
        format!("Partly listening at {}\n", server.url)
    );
    Ok(())
}
