// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;
use crate::store;
use axum::http::StatusCode;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn blocked_password_workers_do_not_hold_storage_or_queue_more_hashes() {
    let path = std::env::temp_dir().join(format!("partly-test-{}", store::secret()));
    let app = App::new(Store::open(path).unwrap());
    let mut releases = Vec::new();
    let mut tasks = Vec::new();
    for _ in 0..2 {
        let (started, ready) = tokio::sync::oneshot::channel();
        let (release, wait) = std::sync::mpsc::channel();
        let auth = app.auth.clone();
        tasks.push(tokio::spawn(async move {
            auth.run(move || {
                started.send(()).unwrap();
                wait.recv().unwrap();
                Ok(())
            })
            .await
        }));
        releases.push(release);
        ready.await.unwrap();
    }
    assert_eq!(
        app.auth.run(|| Ok(())).await.unwrap_err().0,
        StatusCode::TOO_MANY_REQUESTS
    );
    let read = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        app.run(|s| Ok(s.ready())),
    )
    .await;
    // Release workers before asserting so a regression cannot hang test shutdown.
    for release in releases {
        release.send(()).unwrap();
    }
    for task in tasks {
        task.await.unwrap().unwrap();
    }
    assert!(!read.expect("Password work held the store lock").unwrap());
    app.auth.run(|| Ok(())).await.unwrap();
}
