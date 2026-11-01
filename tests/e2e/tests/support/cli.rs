// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::{Result, binary};
use std::{
    process::{Output, Stdio},
    time::Duration,
};
use tokio::{io::AsyncWriteExt, process::Command};

pub async fn command(args: &[&str], password: Option<&str>) -> Result<Output> {
    let mut child = Command::new(binary())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    if let Some(password) = password {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(password.as_bytes())
            .await?;
    }
    drop(child.stdin.take());
    Ok(tokio::time::timeout(Duration::from_secs(10), child.wait_with_output()).await??)
}

pub fn failed(output: &Output, message: &str) {
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(message),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
