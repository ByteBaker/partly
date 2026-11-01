// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tempfile::TempDir;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};

mod packages;
mod source;

use packages::packaged;
pub use packages::{copy_directory, install_node, unpack};
use source::{build_sources, source_command};
pub use source::{go_build_command, verify_python_package};

pub const LANGUAGES: [&str; 4] = ["python", "rust", "go", "node"];
pub fn python() -> String {
    std::env::var("PARTLY_PYTHON").unwrap_or_else(|_| "python3".into())
}

pub async fn output(mut command: Command, seconds: u64) -> Result<std::process::Output> {
    command
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let label = format!("{command:?}");
    tokio::time::timeout(Duration::from_secs(seconds), command.output())
        .await
        .map_err(|_| format!("Command timed out: {label}"))?
        .map_err(Into::into)
}

pub async fn checked(command: Command, seconds: u64) -> Result<std::process::Output> {
    let result = output(command, seconds).await?;
    if !result.status.success() {
        return Err(format!(
            "Command failed ({}):\n{}{}",
            result.status,
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        )
        .into());
    }
    Ok(result)
}

pub struct Driver {
    pub _directory: TempDir,
    pub log_path: PathBuf,
    process: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
}

impl Driver {
    pub fn spawn(mut command: Command, directory: TempDir) -> Result<Self> {
        let log_path = directory.path().join("driver.log");
        let mut process = command
            .kill_on_drop(true)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(File::create(&log_path)?)
            .spawn()?;
        let input = process.stdin.take();
        let output = BufReader::new(process.stdout.take().unwrap());
        Ok(Self {
            _directory: directory,
            log_path,
            process,
            input,
            output,
        })
    }

    pub async fn new(language: &str, openfeature: bool) -> Result<Self> {
        if language == "python" {
            verify_python_package().await?;
        }
        let directory = tempfile::tempdir()?;
        let command = if let Some(packages) = std::env::var_os("PARTLY_SDK_PACKAGES") {
            packaged(
                language,
                directory.path(),
                &fs::canonicalize(packages)?,
                openfeature,
            )
            .await?
        } else {
            build_sources(language, openfeature).await?;
            source_command(language, openfeature)?
        };
        Self::spawn(command, directory)
    }

    pub async fn call(&mut self, op: &str, mut arguments: Value) -> Result<Value> {
        self.call_timeout(op, &mut arguments, Duration::from_secs(10))
            .await
    }

    pub async fn call_timeout(
        &mut self,
        op: &str,
        arguments: &mut Value,
        timeout: Duration,
    ) -> Result<Value> {
        arguments["op"] = op.into();
        let result = tokio::time::timeout(timeout, async {
            let input = self.input.as_mut().ok_or("Driver input closed")?;
            input
                .write_all(format!("{arguments}\n").as_bytes())
                .await
                .map_err(|_| format!("SDK driver exited during {op}"))?;
            input.flush().await?;
            let mut line = vec![];
            loop {
                let buffer = self.output.fill_buf().await?;
                if buffer.is_empty() {
                    return Err(format!("SDK driver exited during {op}").into());
                }
                let end = buffer
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map(|position| position + 1);
                let size = end.unwrap_or(buffer.len());
                if line.len() + size > 65536 {
                    return Err(format!("SDK driver response exceeded 64 KiB during {op}").into());
                }
                line.extend_from_slice(&buffer[..size]);
                self.output.consume(size);
                if end.is_some() {
                    break;
                }
            }
            serde_json::from_slice(&line).map_err(|_| {
                Box::<dyn std::error::Error + Send + Sync>::from(format!(
                    "SDK driver returned invalid JSON during {op}"
                ))
            })
        })
        .await
        .unwrap_or_else(|_| Err(format!("SDK driver timed out during {op}").into()));
        result.map_err(|error| {
            format!(
                "{error}\n{}",
                fs::read_to_string(&self.log_path).unwrap_or_default()
            )
            .into()
        })
    }

    pub async fn simple(&mut self, op: &str, expected: Value) -> Result {
        let result = self.call(op, json!({})).await?;
        assert_eq!(result, expected, "Operation: {op}");
        Ok(())
    }

    pub async fn evaluate(&mut self, flag: &str, context: Value) -> Result<Value> {
        self.call("evaluate", json!({"flag":flag,"context":context}))
            .await
    }

    pub async fn build(
        &mut self,
        server: &Server,
        key: &Value,
        interval: f64,
        trusted: bool,
    ) -> Result {
        let mut args = json!({"url":server.url,"key":key["secret"],"environment":key["environment"],"application_id":key["application_id"],"interval":interval,"timeout":1});
        if trusted {
            args["ca_bundle"] = server.ca.as_ref().map(|p| p.display().to_string()).into();
        }
        assert_eq!(self.call("build", args).await?, json!({"ok":true}));
        Ok(())
    }

    pub async fn finish(&mut self) -> Result {
        drop(self.input.take());
        let result = tokio::time::timeout(Duration::from_secs(3), self.process.wait())
            .await
            .map_err(|_| "SDK driver did not shut down after EOF")??;
        if !result.success() {
            return Err(format!(
                "SDK driver exited with {result}: {}",
                fs::read_to_string(&self.log_path)?
            )
            .into());
        }
        Ok(())
    }
}
