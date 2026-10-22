// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use open_feature::{EvaluationContext, EvaluationContextFieldValue as Field, OpenFeature};
use partly::{openfeature::PartlyProvider, Client};
use serde_json::{json, Value};
use std::{
    io::{self, BufRead, Write},
    sync::Arc,
    time::Duration,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut native: Option<Arc<Client>> = None;
    let mut api = OpenFeature::default();
    for line in io::stdin().lock().lines() {
        let command: Value = serde_json::from_str(&line?)?;
        let result = match command["op"].as_str().unwrap() {
            "build" => {
                native = Some(Arc::new(
                    Client::builder()
                        .url(command["url"].as_str().unwrap())
                        .environment(command["environment"].as_str().unwrap())
                        .application_id(command["application_id"].as_str().unwrap())
                        .secret_key(command["key"].as_str().unwrap())
                        .poll_interval(Duration::from_secs(3600))
                        .build()?,
                ));
                json!({"ok": true})
            }
            "init" => {
                let client = native.as_ref().unwrap();
                client.init().await?;
                api.set_provider(PartlyProvider::new(client.clone())).await;
                json!({"ok": true})
            }
            "evaluate" => {
                let mut context = EvaluationContext::default();
                for (key, value) in command["context"].as_object().unwrap() {
                    if key == "targetingKey" {
                        context = context.with_targeting_key(value.as_str().unwrap());
                        continue;
                    }
                    let field = match value {
                        Value::Bool(v) => Field::Bool(*v),
                        Value::String(v) => Field::String(v.clone()),
                        Value::Number(v) if v.is_i64() => Field::Int(v.as_i64().unwrap()),
                        Value::Number(v) => Field::Float(v.as_f64().unwrap()),
                        _ => Field::new_struct(()),
                    };
                    context = context.with_custom_field(key, field);
                }
                match api
                    .create_client()
                    .get_bool_details(command["flag"].as_str().unwrap(), Some(&context), None)
                    .await
                {
                    Ok(value) => {
                        json!({"value": value.value, "error": "", "variant": value.variant, "reason": value.reason.unwrap().to_string()})
                    }
                    Err(error) => json!({"error": error.code.to_string()}),
                }
            }
            "refresh" => match native.as_ref().unwrap().refresh().await {
                Ok(updated) => json!({"updated": updated}),
                Err(error) => json!({"error": error.to_string()}),
            },
            "close" => {
                api.shutdown().await;
                native.as_ref().unwrap().close().await?;
                json!({"ok": true})
            }
            _ => panic!("unknown operation"),
        };
        println!("{result}");
        io::stdout().flush()?;
    }
    api.shutdown().await;
    if let Some(client) = native {
        client.close().await?;
    }
    Ok(())
}
