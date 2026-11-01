// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::support::*;
use reqwest::Method;
use serde_json::json;
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
        admin.get("/api/environments/development/apps").await?["applications"][0]["keys"][0]["secret"],
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
async fn earlier_development_schema_is_rejected_without_changes() -> Result {
    for sql in [
        "ALTER TABLE sdk_keys DROP COLUMN name",
        "ALTER TABLE users RENAME TO admin",
        "DROP TABLE permissions",
        "DROP TABLE flags; CREATE TABLE flags (environment TEXT,key TEXT,enabled INTEGER,percentage INTEGER)",
    ] {
        let (mut server, admin) = fixture().await?;
        admin.key("development").await?;
        server.stop().await?;
        server.database()?.execute_batch(sql)?;
        let before = fs::read(&server.db)?;
        assert!(server.start().await.is_err());
        assert!(server.log().contains("incompatible development schema"));
        assert_eq!(fs::read(&server.db)?, before);
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
            assert!(
                response.headers()["content-type"]
                    .to_str()?
                    .starts_with("text/html")
            );
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
        assert!(
            response.headers()["content-security-policy"]
                .to_str()?
                .contains("frame-ancestors 'none'")
        );
        assert!(status(response, expected).await?["error"].is_string());
    }
    Ok(())
}
