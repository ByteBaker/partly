// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::support::{driver::*, *};
use reqwest::Method;
use rustls::pki_types::{pem::PemObject, CertificateDer};
use serde_json::json;
use std::{fs, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

#[tokio::test]
async fn setup_cookies_origins_and_hostname() -> Result {
    let mut server = Server::new()?;
    let tls = Tls::new("127.0.0.1", false)?;
    tls.configure(&mut server);
    server
        .extra
        .extend(["--hostname".into(), "partly.test".into()]);
    server.start().await?;
    let admin = server.api(true)?;
    assert_eq!(admin.get("/api/status").await?["setup_required"], true);
    assert!(!server.db.exists());
    let credentials = json!({"username":"admin","password":PASSWORD});
    let setup = admin
        .request(Method::POST, "/api/setup")
        .header("Origin", &server.url)
        .json(&credentials)
        .send()
        .await?;
    let cookie = setup.headers()["set-cookie"].to_str()?.to_owned();
    for attribute in ["Secure", "HttpOnly", "SameSite=Strict"] {
        assert!(cookie.contains(attribute));
    }
    status(setup, 200).await?;
    assert_eq!(admin.get("/api/status").await?["authenticated"], true);
    let key = admin.key("development").await?;
    for headers in [
        vec![("Origin", server.url.replace("https:", "http:"))],
        vec![("Origin", "https://other.example".into())],
        vec![
            ("Origin", server.url.replace("https:", "http:")),
            ("X-Forwarded-Proto", "http".into()),
        ],
        vec![
            ("Origin", server.url.replace("https:", "http:")),
            (
                "Forwarded",
                format!("proto=http;host=\"127.0.0.1:{}\"", server.port),
            ),
        ],
    ] {
        let mut request = admin
            .request(Method::POST, "/api/environments/development/apps")
            .json(&json!({"name":"Rejected"}));
        for (name, value) in headers {
            request = request.header(name, value);
        }
        let response = request.send().await?;
        assert_eq!(response.headers()["cache-control"], "no-store");
        status(response, 403).await?;
    }
    assert_eq!(
        admin.get("/api/environments/development/apps").await?["applications"][0]["keys"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    status(
        admin
            .request(Method::GET, "/api/status")
            .header("Host", format!("partly.test:{}", server.port))
            .send()
            .await?,
        200,
    )
    .await?;
    status(
        admin
            .request(Method::GET, "/api/status")
            .header("Host", format!("partly.test:{}", server.port))
            .header("Origin", format!("https://partly.test:{}", server.port))
            .send()
            .await?,
        200,
    )
    .await?;
    status(
        admin
            .request(Method::GET, "/api/status")
            .header("Host", "unknown.example")
            .header("X-Forwarded-Host", "partly.test")
            .send()
            .await?,
        421,
    )
    .await?;
    let logout = admin
        .request(Method::POST, "/api/logout")
        .header("Origin", &server.url)
        .send()
        .await?;
    assert!(logout.headers()["set-cookie"].to_str()?.contains("Secure"));
    status(logout, 204).await?;
    status(
        admin
            .request(Method::GET, "/api/environments")
            .send()
            .await?,
        401,
    )
    .await?;
    status(
        admin
            .request(Method::GET, "/api/environments")
            .header("Cookie", cookie.split(';').next().unwrap())
            .send()
            .await?,
        401,
    )
    .await?;
    let login = admin
        .request(Method::POST, "/api/login")
        .header("Origin", &server.url)
        .json(&credentials)
        .send()
        .await?;
    assert!(login.headers()["set-cookie"].to_str()?.contains("Secure"));
    status(login, 200).await?;
    admin.get("/api/environments").await?;
    assert!(!server.log().contains(text(&key["secret"])));
    assert!(reqwest::Client::new()
        .get(server.url.replace("https:", "http:"))
        .timeout(Duration::from_secs(1))
        .send()
        .await
        .is_err());
    Ok(())
}

#[tokio::test]
async fn verified_tls_12_and_13() -> Result {
    let mut server = Server::new()?;
    let tls = Tls::new("127.0.0.1", false)?;
    tls.configure(&mut server);
    server.start().await?;
    let _ = rustls::crypto::ring::default_provider().install_default();
    for version in [&rustls::version::TLS12, &rustls::version::TLS13] {
        let mut roots = rustls::RootCertStore::empty();
        for cert in CertificateDer::pem_slice_iter(&fs::read(&tls.ca)?) {
            roots.add(cert?)?;
        }
        let config = rustls::ClientConfig::builder_with_protocol_versions(&[version])
            .with_root_certificates(roots)
            .with_no_client_auth();
        let socket = tokio::net::TcpStream::connect(("127.0.0.1", server.port)).await?;
        let mut stream = tokio_rustls::TlsConnector::from(Arc::new(config))
            .connect("127.0.0.1".try_into()?, socket)
            .await?;
        assert_eq!(stream.get_ref().1.protocol_version(), Some(version.version));
        stream
            .write_all(
                format!(
                    "GET /api/status HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
                    server.port
                )
                .as_bytes(),
            )
            .await?;
        let mut response = vec![];
        tokio::time::timeout(Duration::from_secs(3), stream.read_to_end(&mut response)).await??;
        assert!(response.starts_with(b"HTTP/1.1 200"));
    }
    Ok(())
}

#[tokio::test]
async fn invalid_tls_configuration_creates_no_database() -> Result {
    for problem in [
        "missing_cert",
        "missing_key",
        "malformed_cert",
        "malformed_key",
        "mismatched_key",
    ] {
        let server = Server::new()?;
        let tls = Tls::new("127.0.0.1", false)?;
        match problem {
            "malformed_cert" => fs::write(&tls.cert, "not a PEM certificate")?,
            "malformed_key" => fs::write(&tls.key, "not a PEM private key")?,
            "mismatched_key" => fs::write(&tls.key, rcgen::KeyPair::generate()?.serialize_pem())?,
            _ => {}
        }
        let mut command = Command::new(binary());
        command
            .args(["serve", "--port", "0", "--db"])
            .arg(&server.db);
        if problem != "missing_cert" {
            command.arg("--tls-cert").arg(&tls.cert);
        }
        if problem != "missing_key" {
            command.arg("--tls-key").arg(&tls.key);
        }
        let result = output(command, 10).await?;
        assert!(!result.status.success() && !result.stderr.is_empty());
        assert!(!server.db.exists());
        assert!(!String::from_utf8_lossy(&result.stderr).contains(&fs::read_to_string(&tls.key)?));
    }
    Ok(())
}

#[tokio::test]
async fn sdk_rejects_tls_downgrade_and_invalid_certificates() -> Result {
    for problem in ["wrong_host", "expired", "untrusted_issuer"] {
        let mut server = Server::new()?;
        let tls = Tls::new("127.0.0.1", false)?;
        tls.configure(&mut server);
        server.start().await?;
        let admin = server.setup().await?;
        admin
            .flag("development", "new_checkout", json!({"mode":"enabled"}))
            .await?;
        let key = admin.key("development").await?;
        let mut warm = Driver::new("python", false).await?;
        let mut cold = Driver::new("python", false).await?;
        warm.build(&server, &key, 3600.0, true).await?;
        warm.simple("init", json!({"ok":true})).await?;
        warm.simple("checkpoint", json!({"ok":true})).await?;
        cold.build(&server, &key, 3600.0, true).await?;
        server.stop().await?;
        match problem {
            "wrong_host" => tls.rotate("different.example", false)?,
            "expired" => tls.rotate("127.0.0.1", true)?,
            _ => {
                let other = Tls::new("127.0.0.1", false)?;
                fs::copy(other.cert, &tls.cert)?;
                fs::copy(other.key, &tls.key)?;
            }
        }
        server.start_tcp().await?;
        cold.simple("init", json!({"error":"refresh"})).await?;
        assert_eq!(cold.call("state", json!({})).await?["polling"], false);
        assert_eq!(
            cold.evaluate("new_checkout", json!({})).await?,
            json!({"error":"policy_not_ready"})
        );
        warm.simple("refresh", json!({"error":"refresh"})).await?;
        warm.simple("assert_preserved", json!({"ok":true})).await?;
        assert_eq!(
            warm.evaluate("new_checkout", json!({})).await?,
            json!({"decision":"enabled"})
        );
        for driver in [&mut warm, &mut cold] {
            driver.simple("close", json!({"ok":true})).await?;
            driver.finish().await?;
            assert!(!fs::read_to_string(&driver.log_path)?.contains(text(&key["secret"])));
        }
    }
    let (mut server, admin) = fixture().await?;
    let key = admin.key("development").await?;
    admin
        .flag("development", "new_checkout", json!({"mode":"enabled"}))
        .await?;
    server.url = server.url.replace("http:", "https:");
    let mut client = Driver::new("python", false).await?;
    client.build(&server, &key, 3600.0, false).await?;
    client.simple("init", json!({"error":"refresh"})).await?;
    assert_eq!(
        client.evaluate("new_checkout", json!({})).await?,
        json!({"error":"policy_not_ready"})
    );
    client.simple("close", json!({"ok":true})).await?;
    client.finish().await
}

#[tokio::test]
async fn sdk_trust_is_explicit_and_outage_preserves_cache() -> Result {
    let mut server = Server::new()?;
    let tls = Tls::new("127.0.0.1", false)?;
    tls.configure(&mut server);
    server.start().await?;
    let admin = server.setup().await?;
    let key = admin.key("development").await?;
    admin
        .flag("development", "new_checkout", json!({"mode":"enabled"}))
        .await?;
    verify_python_package().await?;
    let mut command = Command::new(python());
    command
        .arg("-I")
        .arg(root().join("tests/sdk_contract/python/driver.py"))
        .env("SSL_CERT_FILE", &tls.ca);
    let mut untrusted = Driver::spawn(command, tempfile::tempdir()?)?;
    untrusted.build(&server, &key, 3600.0, false).await?;
    untrusted.simple("init", json!({"error":"refresh"})).await?;
    assert_eq!(
        untrusted.evaluate("new_checkout", json!({})).await?,
        json!({"error":"policy_not_ready"})
    );
    untrusted.simple("close", json!({"ok":true})).await?;
    untrusted.finish().await?;
    let mut trusted = Driver::new("python", false).await?;
    trusted.build(&server, &key, 3600.0, true).await?;
    trusted.simple("init", json!({"ok":true})).await?;
    assert_eq!(
        trusted.evaluate("new_checkout", json!({})).await?,
        json!({"decision":"enabled"})
    );
    trusted.simple("refresh", json!({"updated":false})).await?;
    admin
        .flag("development", "new_checkout", json!({"mode":"disabled"}))
        .await?;
    trusted.simple("refresh", json!({"updated":true})).await?;
    assert_eq!(
        trusted.evaluate("new_checkout", json!({})).await?,
        json!({"decision":"disabled"})
    );
    trusted.simple("checkpoint", json!({"ok":true})).await?;
    server.stop().await?;
    trusted
        .simple("refresh", json!({"error":"refresh"}))
        .await?;
    trusted
        .simple("assert_preserved", json!({"ok":true}))
        .await?;
    assert_eq!(
        trusted.evaluate("new_checkout", json!({})).await?,
        json!({"decision":"disabled"})
    );
    trusted.simple("close", json!({"ok":true})).await?;
    trusted.finish().await
}
