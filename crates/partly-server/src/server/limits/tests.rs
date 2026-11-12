// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;
use axum_server::accept::DefaultAcceptor;

async fn stream() -> (TcpStream, TcpStream) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let peer = TcpStream::connect(listener.local_addr().unwrap())
        .await
        .unwrap();
    (listener.accept().await.unwrap().0, peer)
}

#[tokio::test]
async fn connection_slots_are_shared_and_released_when_stream_drops() {
    let acceptor = LimitedAcceptor {
        inner: DefaultAcceptor,
        slots: Arc::new(Semaphore::new(1)),
    };
    let (socket, _peer) = stream().await;
    let (connection, ()) = acceptor.accept(socket, ()).await.unwrap();
    let (socket, _other_peer) = stream().await;
    assert!(acceptor.clone().accept(socket, ()).await.is_err());
    drop(connection);
    let (socket, _last_peer) = stream().await;
    assert!(acceptor.accept(socket, ()).await.is_ok());
}

#[tokio::test]
async fn failed_handshakes_release_connection_slots() {
    #[derive(Clone)]
    struct Reject;
    impl Accept<TcpStream, ()> for Reject {
        type Stream = TcpStream;
        type Service = ();
        type Future = std::future::Ready<io::Result<(TcpStream, ())>>;
        fn accept(&self, _: TcpStream, _: ()) -> Self::Future {
            std::future::ready(Err(io::Error::other("Handshake rejected")))
        }
    }
    let acceptor = LimitedAcceptor::new(Reject);
    let (socket, _peer) = stream().await;
    assert!(acceptor.accept(socket, ()).await.is_err());
    assert_eq!(acceptor.slots.available_permits(), MAX_CONNECTIONS);
}

#[tokio::test]
async fn request_slots_reject_before_reading_a_body_and_recover() {
    use axum::{Router, middleware, routing::post};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let slots = Arc::new(Semaphore::new(0));
    let router = Router::new()
        .route("/", post(|| async { "accepted" }))
        .layer(middleware::from_fn_with_state(slots.clone(), request));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    for (body, expected) in [("", "503 Service Unavailable"), ("x", "200 OK")] {
        let mut stream = TcpStream::connect(address).await.unwrap();
        stream.write_all(format!("POST / HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1\r\nConnection: close\r\n\r\n{body}").as_bytes()).await.unwrap();
        let mut response = String::new();
        let read =
            tokio::time::timeout(Duration::from_secs(2), stream.read_to_string(&mut response))
                .await;
        if !matches!(read, Ok(Ok(_))) {
            server.abort();
        }
        read.unwrap().unwrap();
        assert!(response.contains(expected), "{response}");
        if body.is_empty() {
            assert!(response.to_ascii_lowercase().contains("retry-after: 1"));
            slots.add_permits(1);
        } else {
            assert!(response.contains("accepted"));
        }
    }
    server.abort();
}
