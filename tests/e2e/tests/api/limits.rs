// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::support::{Result, fixture, status};
use reqwest::Method;
use serde_json::json;
use std::time::Duration;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};

#[tokio::test]
async fn malformed_chunked_body_is_not_reported_as_oversized() -> Result {
    let (server, _admin) = fixture().await?;
    let mut socket = TcpStream::connect(("127.0.0.1", server.port)).await?;
    socket.write_all(format!("POST /api/login HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nX-Partly-Request: dashboard\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n\r\nnot-hex\r\n", server.port).as_bytes()).await?;
    let mut response = String::new();
    tokio::time::timeout(Duration::from_secs(3), socket.read_to_string(&mut response)).await??;
    assert!(response.starts_with("HTTP/1.1 400"), "{response}");
    Ok(())
}

#[tokio::test]
async fn incomplete_login_bodies_expire_even_when_data_keeps_arriving() -> Result {
    let (server, admin) = fixture().await?;
    let socket = TcpStream::connect(("127.0.0.1", server.port)).await?;
    let (read, mut write) = socket.into_split();
    let mut read = BufReader::new(read);
    write.write_all(format!("POST /api/login HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nX-Partly-Request: dashboard\r\nContent-Type: application/json\r\nContent-Length: 16000\r\nExpect: 100-continue\r\n\r\n", server.port).as_bytes()).await?;
    let mut interim = String::new();
    tokio::time::timeout(Duration::from_secs(3), read.read_line(&mut interim)).await??;
    assert!(interim.starts_with("HTTP/1.1 100"), "{interim}");
    let dripping = tokio::spawn(async move {
        for _ in 0..15 {
            if write.write_all(b" ").await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
    let mut response = String::new();
    let result =
        tokio::time::timeout(Duration::from_secs(13), read.read_to_string(&mut response)).await;
    dripping.abort();
    result??;
    assert!(response.contains("408 Request Timeout"), "{response}");
    assert!(response.to_ascii_lowercase().contains("connection: close"));
    assert!(
        response
            .to_ascii_lowercase()
            .contains("cache-control: no-store")
    );
    status(admin.request(Method::GET, "/api/status").send().await?, 200).await?;
    Ok(())
}

#[tokio::test]
async fn expensive_rules_are_rejected_without_affecting_other_environments() -> Result {
    let (_server, admin) = fixture().await?;
    let before = admin.get("/api/environments/development/flags").await?;
    let groups = vec![vec![json!({"k":"id","op":"regex","v":r"\w{50}"}); 32]];
    admin
        .post(
            "/api/environments/development/flags",
            json!({"key":"expensive","mode":"custom","action":"enable","groups":groups}),
            422,
        )
        .await?;
    assert_eq!(
        admin.get("/api/environments/development/flags").await?,
        before
    );
    admin.get("/api/environments/production/flags").await?;
    admin.post("/api/environments/development/flags", json!({"key":"normal","mode":"custom","action":"enable","groups":[[{"k":"id","op":"regex","v":"^user-[0-9]+$"}]]}), 201).await?;
    Ok(())
}
