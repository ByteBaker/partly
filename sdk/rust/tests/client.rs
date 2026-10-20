// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use partly::{Client, Context, Error};
use std::time::Duration;

#[test]
fn build_validates_without_a_runtime_or_network() {
    assert!(Client::builder()
        .url("https://127.0.0.1:1")
        .secret_key("key")
        .build()
        .is_ok());
    for url in [
        "ftp://host",
        "http://name:password@host",
        "http://@host",
        "http://host/path",
        "http://host?x",
        "http://host#x",
        " http://host",
        "http://host\\path",
    ] {
        assert!(
            Client::builder()
                .url(url)
                .secret_key("key")
                .build()
                .is_err(),
            "{url}"
        );
    }
    assert!(Client::builder()
        .url("http://localhost")
        .secret_key("key\n")
        .build()
        .is_err());
    assert!(Client::builder()
        .url("http://localhost")
        .secret_key("key")
        .timeout(Duration::ZERO)
        .build()
        .is_err());
}

#[tokio::test]
async fn lifecycle_errors_do_not_need_a_policy() {
    let client = Client::builder()
        .url("http://127.0.0.1:1")
        .secret_key("key")
        .build()
        .unwrap();
    assert!(matches!(
        client.evaluate("flag", &Context::new()),
        Err(Error::PolicyNotReady)
    ));
    assert!(matches!(client.refresh().await, Err(Error::PolicyNotReady)));
    client.close().await.unwrap();
    client.close().await.unwrap();
    assert!(matches!(client.init().await, Err(Error::ClientClosed)));
    assert!(matches!(client.refresh().await, Err(Error::ClientClosed)));
}

#[tokio::test]
async fn closing_cancels_an_initial_download_without_waiting_for_timeout() {
    use tokio::{io::AsyncReadExt, net::TcpListener, sync::oneshot, time::timeout};
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (seen, received) = oneshot::channel();
    let peer = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buffer = [0; 2048];
        assert!(socket.read(&mut buffer).await.unwrap() > 0);
        seen.send(()).unwrap();
        socket.read(&mut buffer).await.unwrap()
    });
    let client = std::sync::Arc::new(
        Client::builder()
            .url(format!("http://{address}"))
            .secret_key("key")
            .build()
            .unwrap(),
    );
    // The separate request task lets close run while init is waiting for a response.
    let requesting = client.clone();
    let init = tokio::spawn(async move { requesting.init().await });
    timeout(Duration::from_secs(2), received)
        .await
        .unwrap()
        .unwrap();
    timeout(Duration::from_secs(1), client.close())
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(init.await.unwrap(), Err(Error::ClientClosed)));
    peer.abort();
}

#[tokio::test]
async fn invalid_policy_errors_redact_response_values_and_retain_policy_and_etag() {
    use partly::Decision;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
        time::{sleep, timeout},
    };

    const SENTINEL: &str = "private-response-value";
    let policy = serde_json::json!({
        "schema_version": 1, "project": "default", "environment": "development",
        "revision": 1, "flags": {"flag": {"mode": "enabled"}}
    });
    let accepted_body = policy.to_string();
    let mut invalid = policy;
    invalid["flags"]["flag"]["mode"] = SENTINEL.into();
    let rejected_body = invalid.to_string();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let peer = tokio::spawn(async move {
        for index in 0.. {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buffer = [0; 1024];
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let count = socket.read(&mut buffer).await.unwrap();
                assert!(count > 0);
                request.extend_from_slice(&buffer[..count]);
            }
            let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
            let (body, etag) = if index == 0 {
                assert!(!request.contains("if-none-match:"));
                (&accepted_body, "accepted")
            } else {
                assert!(request.contains("if-none-match: \"accepted\"\r\n"));
                (&rejected_body, "rejected")
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nETag: \"{etag}\"\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        }
    });
    let client = Client::builder()
        .url(format!("http://{address}"))
        .secret_key("key")
        .poll_interval(Duration::from_millis(20))
        .timeout(Duration::from_secs(1))
        .build()
        .unwrap();
    let assert_redacted = |error: &Error| {
        assert!(matches!(error, Error::InvalidPolicy(_)));
        assert!(!error.to_string().contains(SENTINEL));
        assert!(!format!("{error:?}").contains(SENTINEL));
    };

    client.init().await.unwrap();
    assert_redacted(&client.refresh().await.unwrap_err());
    assert_eq!(
        client.evaluate("flag", &Context::new()).unwrap(),
        Decision::Enabled
    );
    let background_error = timeout(Duration::from_secs(2), async {
        loop {
            if let Some(error) = client.last_refresh_error() {
                break error;
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_redacted(&background_error);
    // This request also verifies the accepted ETag survived the background failure.
    assert_redacted(&client.refresh().await.unwrap_err());
    assert_redacted(&client.last_refresh_error().unwrap());
    client.close().await.unwrap();
    assert_eq!(
        client.evaluate("flag", &Context::new()).unwrap(),
        Decision::Enabled
    );
    peer.abort();
    assert!(peer.await.unwrap_err().is_cancelled());
}
