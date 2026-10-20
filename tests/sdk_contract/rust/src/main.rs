// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use partly::{Client, Context, Decision, Error, EvaluationError};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn error_response(error: Error) -> Value {
    let code = match error {
        Error::Evaluation(EvaluationError::FlagNotFound(_)) => "flag_not_found",
        Error::Evaluation(EvaluationError::InvalidContext { .. }) => "invalid_context",
        Error::Evaluation(EvaluationError::ContextType {
            flag_key,
            k,
            op,
            expected_type,
            actual_type,
        }) => {
            return json!({
                "error": "context_type", "flag_key": flag_key, "k": k, "op": op,
                "expected_type": expected_type, "actual_type": actual_type,
            });
        }
        Error::PolicyNotReady => "policy_not_ready",
        Error::Authentication => "authentication",
        Error::InvalidPolicy(_) => "invalid_policy",
        Error::Refresh(_) => "refresh",
        Error::ClientClosed => "client_closed",
        other => panic!("Unexpected SDK error: {other}"),
    };
    json!({"error": code})
}

async fn execute(client: &mut Option<Client>, command: Value) -> Result<Value, Error> {
    let op = command["op"].as_str().expect("operation");
    if op == "build" {
        let mut builder = Client::builder()
            .url(command["url"].as_str().expect("url"))
            .secret_key(command["key"].as_str().expect("key"))
            .poll_interval(Duration::from_secs_f64(
                command["interval"].as_f64().expect("interval"),
            ))
            .timeout(Duration::from_secs_f64(
                command["timeout"].as_f64().expect("timeout"),
            ));
        if let Some(path) = command["ca_bundle"].as_str() {
            builder = builder.ca_bundle(path);
        }
        *client = Some(builder.build()?);
        return Ok(json!({"ok": true}));
    }
    let client = client.as_ref().expect("build a client first");
    match op {
        "init" => client.init().await?,
        "evaluate" => {
            let context: Context =
                serde_json::from_value(command["context"].clone()).expect("context map");
            let decision =
                client.evaluate(command["flag"].as_str().expect("flag key"), &context)?;
            return Ok(json!({"decision": match decision {
                Decision::Enabled => "enabled",
                Decision::Disabled => "disabled",
            }}));
        }
        "refresh" => return Ok(json!({"updated": client.refresh().await?})),
        "close" => client.close().await?,
        _ => panic!("Unknown contract operation: {op}"),
    }
    Ok(json!({"ok": true}))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = None;
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut output = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        let response = match execute(&mut client, serde_json::from_str(&line)?).await {
            Ok(value) => value,
            Err(error) => error_response(error),
        };
        output.write_all(response.to_string().as_bytes()).await?;
        output.write_all(b"\n").await?;
        output.flush().await?;
    }
    if let Some(client) = client {
        client.close().await?;
    }
    Ok(())
}
