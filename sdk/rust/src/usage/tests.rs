// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::{Client, Context, Decision};
use std::sync::{atomic::AtomicBool, RwLock};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::{mpsc, oneshot},
    time::timeout,
};

struct Report {
    flags: Vec<String>,
    reply: oneshot::Sender<u16>,
}

struct Server {
    client: Arc<Client>,
    reports: mpsc::UnboundedReceiver<Report>,
    policy: Arc<RwLock<String>>,
    block_refresh: Arc<AtomicBool>,
    refreshes: mpsc::UnboundedReceiver<oneshot::Sender<u16>>,
    task: JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn policy(count: usize) -> String {
    let mut flags = serde_json::Map::new();
    for i in 0..count {
        flags.insert(
            format!("flag_{i:03}"),
            serde_json::json!({"mode":"disabled"}),
        );
    }
    flags.insert(
        "context".into(),
        serde_json::json!({"mode":"percentage", "percentage":50}),
    );
    serde_json::json!({
        "schema_version":1, "application_id":"app_test", "project":"default",
        "environment":"development", "revision":1, "flags":flags
    })
    .to_string()
}

async fn client(count: usize) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let policy = Arc::new(RwLock::new(policy(count)));
    let block_refresh = Arc::new(AtomicBool::new(false));
    let (send_reports, reports) = mpsc::unbounded_channel();
    let (send_refreshes, refreshes) = mpsc::unbounded_channel();
    let body = Arc::clone(&policy);
    let block = Arc::clone(&block_refresh);
    let task = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let send = send_reports.clone();
            let send_refresh = send_refreshes.clone();
            let body = Arc::clone(&body);
            let block = Arc::clone(&block);
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                let mut buffer = [0; 2048];
                let boundary = loop {
                    if let Some(index) = bytes.windows(4).position(|part| part == b"\r\n\r\n") {
                        break index + 4;
                    }
                    let count = socket.read(&mut buffer).await.unwrap();
                    if count == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&buffer[..count]);
                };
                let headers = String::from_utf8(bytes[..boundary].to_vec())
                    .unwrap()
                    .to_ascii_lowercase();
                assert!(headers.contains("authorization: bearer key\r\n"));
                assert!(headers.contains("x-partly-environment: development\r\n"));
                assert!(headers.contains("x-partly-application: app_test\r\n"));
                let (status, body) = if headers.starts_with("get /api/policy ") {
                    let body = body.read().unwrap().clone();
                    if block.load(Ordering::Acquire) {
                        let (reply, wait) = oneshot::channel();
                        send_refresh.send(reply).unwrap();
                        let Ok(status) = wait.await else { return };
                        if status == 0 {
                            return;
                        }
                        (status, body)
                    } else {
                        (200, body)
                    }
                } else {
                    assert!(headers.starts_with("post /api/usage "));
                    assert!(headers.contains("content-type: application/json\r\n"));
                    let length: usize = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length: "))
                        .unwrap()
                        .parse()
                        .unwrap();
                    while bytes.len() < boundary + length {
                        let count = socket.read(&mut buffer).await.unwrap();
                        assert_ne!(count, 0);
                        bytes.extend_from_slice(&buffer[..count]);
                    }
                    let value: serde_json::Value =
                        serde_json::from_slice(&bytes[boundary..]).unwrap();
                    let flags: Vec<String> =
                        serde_json::from_value(value["flags"].clone()).unwrap();
                    assert!(flags.len() <= BATCH_SIZE);
                    let (reply, wait) = oneshot::channel();
                    if send.send(Report { flags, reply }).is_err() {
                        return;
                    }
                    let Ok(status) = wait.await else { return };
                    (status, String::new())
                };
                let revocation = if status == 401 {
                    "X-Partly-Error: key_revoked\r\n"
                } else {
                    ""
                };
                let response = format!(
                    "HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nETag: \"one\"\r\n{revocation}Connection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    let client = Arc::new(
        Client::builder()
            .url(format!("http://{address}"))
            .environment("development")
            .application_id("app_test")
            .secret_key("key")
            .poll_interval(Duration::from_secs(3600))
            .timeout(Duration::from_secs(1))
            .build()
            .unwrap(),
    );
    client.init().await.unwrap();
    Server {
        client,
        reports,
        policy,
        block_refresh,
        refreshes,
        task,
    }
}

impl Server {
    async fn next(&mut self) -> Report {
        timeout(Duration::from_secs(2), self.reports.recv())
            .await
            .unwrap()
            .unwrap()
    }

    fn due(&self) {
        self.client.inner.usage.lock().unwrap().last = Instant::now() - INTERVAL;
    }

    async fn finish(&self, report: Report, status: u16) {
        report.reply.send(status).unwrap();
        timeout(Duration::from_secs(2), async {
            loop {
                if self
                    .client
                    .inner
                    .usage
                    .lock()
                    .unwrap()
                    .task
                    .as_ref()
                    .is_some_and(JoinHandle::is_finished)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn lazy_known_failures_deduplicate_and_off_thread_evaluation_reports() {
    let mut server = client(1).await;
    for _ in 0..10 {
        assert_eq!(
            server.client.evaluate("flag_000", &Context::new()).unwrap(),
            Decision::Disabled
        );
    }
    assert!(server.client.evaluate("context", &Context::new()).is_err());
    assert!(server.client.evaluate("missing", &Context::new()).is_err());
    assert!(server.reports.try_recv().is_err());
    server.due();
    assert!(server.client.has_flag("flag_000").unwrap());
    assert!(server.reports.try_recv().is_err());
    let client = Arc::clone(&server.client);
    std::thread::spawn(move || client.evaluate("flag_000", &Context::new()).unwrap())
        .join()
        .unwrap();
    let report = server.next().await;
    assert_eq!(report.flags, ["flag_000", "context"]);
    server.client.evaluate("flag_000", &Context::new()).unwrap();
    server.finish(report, 204).await;
    assert_eq!(
        server.client.inner.usage.lock().unwrap().pending,
        ["flag_000"]
    );
    server.client.close().await.unwrap();
}

#[tokio::test]
async fn bounded_single_flight_fifo_and_fresh_tail() {
    let mut server = client(300).await;
    for i in 0..300 {
        server
            .client
            .evaluate(&format!("flag_{i:03}"), &Context::new())
            .unwrap();
    }
    let report = server.next().await;
    assert_eq!(report.flags.len(), 128);
    assert_eq!(report.flags[0], "flag_000");
    assert_eq!(report.flags[127], "flag_127");
    for _ in 0..30 {
        server.client.evaluate("flag_000", &Context::new()).unwrap();
    }
    assert!(server.reports.try_recv().is_err());
    server.finish(report, 204).await;
    assert!(server.reports.try_recv().is_err());
    server.client.evaluate("flag_000", &Context::new()).unwrap();
    let report = server.next().await;
    assert_eq!(report.flags.len(), 128);
    assert_eq!(report.flags[0], "flag_128");
    server.finish(report, 204).await;
    server.client.evaluate("flag_000", &Context::new()).unwrap();
    assert!(server
        .client
        .inner
        .usage
        .lock()
        .unwrap()
        .task
        .as_ref()
        .unwrap()
        .is_finished());
    assert!(server.reports.try_recv().is_err());
    server.due();
    server.client.evaluate("flag_000", &Context::new()).unwrap();
    let report = server.next().await;
    assert_eq!(report.flags.len(), 45);
    assert_eq!(report.flags[0], "flag_256");
    assert_eq!(report.flags[44], "flag_000");
    server.finish(report, 204).await;
    server.client.close().await.unwrap();
}

#[tokio::test]
async fn overdue_partial_tail_does_not_wait_another_interval() {
    let mut server = client(300).await;
    for i in 0..300 {
        server
            .client
            .evaluate(&format!("flag_{i:03}"), &Context::new())
            .unwrap();
    }
    let report = server.next().await;
    server.finish(report, 204).await;
    server.due();
    server.client.evaluate("flag_000", &Context::new()).unwrap();
    let report = server.next().await;
    assert_eq!(report.flags.len(), 128);
    server.finish(report, 204).await;
    server.client.evaluate("flag_000", &Context::new()).unwrap();
    let report = server.next().await;
    assert_eq!(report.flags.len(), 45);
    assert_eq!(report.flags[0], "flag_256");
    server.finish(report, 204).await;
    server.client.close().await.unwrap();
}

#[tokio::test]
async fn failure_backs_off_and_prunes_names_after_refresh() {
    let mut server = client(130).await;
    for i in 0..130 {
        server
            .client
            .evaluate(&format!("flag_{i:03}"), &Context::new())
            .unwrap();
    }
    let report = server.next().await;
    let failed = report.flags.clone();
    server.finish(report, 500).await;
    server.client.evaluate("flag_000", &Context::new()).unwrap();
    assert!(server.reports.try_recv().is_err());
    assert!(server.client.last_refresh_error().is_none());
    server.due();
    server.client.evaluate("flag_000", &Context::new()).unwrap();
    let report = server.next().await;
    assert_eq!(report.flags, failed);
    *server.policy.write().unwrap() = policy(1);
    server.client.refresh().await.unwrap();
    server.finish(report, 500).await;
    {
        let usage = server.client.inner.usage.lock().unwrap();
        assert_eq!(usage.pending, ["flag_000"]);
        assert_eq!(usage.names.len(), 1);
    }
    server.due();
    server.client.evaluate("flag_000", &Context::new()).unwrap();
    let report = server.next().await;
    server.finish(report, 204).await;
    server.client.close().await.unwrap();
}

#[tokio::test]
async fn reporting_revocation_wins_over_a_concurrent_refresh() {
    for status in [200, 500, 0] {
        let mut server = client(1).await;
        server.block_refresh.store(true, Ordering::Release);
        let client = Arc::clone(&server.client);
        let refreshing = tokio::spawn(async move { client.refresh().await });
        let release = timeout(Duration::from_secs(2), server.refreshes.recv())
            .await
            .unwrap()
            .unwrap();
        server.due();
        server.client.evaluate("flag_000", &Context::new()).unwrap();
        let report = server.next().await;
        server.finish(report, 401).await;
        release.send(status).unwrap();
        assert!(matches!(refreshing.await.unwrap(), Err(Error::KeyRevoked)));
        assert!(matches!(
            server.client.evaluate("flag_000", &Context::new()),
            Err(Error::KeyRevoked)
        ));
        assert!(matches!(server.client.init().await, Err(Error::KeyRevoked)));
        assert!(server.client.inner.accepted.read().unwrap().is_none());
        assert!(server.client.inner.usage.lock().unwrap().pending.is_empty());
        assert!(matches!(
            server.client.last_refresh_error().as_deref(),
            Some(Error::KeyRevoked)
        ));
        server.client.close().await.unwrap();
    }
}

#[tokio::test]
async fn close_cancels_reporting_without_flushing_and_keeps_cached_evaluation() {
    let mut server = client(1).await;
    server.due();
    server.client.evaluate("flag_000", &Context::new()).unwrap();
    let report = server.next().await;
    server.client.evaluate("flag_000", &Context::new()).unwrap();
    timeout(Duration::from_millis(250), server.client.close())
        .await
        .unwrap()
        .unwrap();
    drop(report);
    assert_eq!(
        server.client.evaluate("flag_000", &Context::new()).unwrap(),
        Decision::Disabled
    );
    assert!(server.client.inner.usage.lock().unwrap().pending.is_empty());
    assert!(server.reports.try_recv().is_err());
    server.client.close().await.unwrap();
}
