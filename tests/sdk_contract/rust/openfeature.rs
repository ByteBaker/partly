// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use open_feature::{EvaluationContext, EvaluationContextFieldValue as Field, OpenFeature};
use partly::{Client, openfeature::PartlyProvider};
use serde_json::{Value, json};
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
            "evaluate" | "resolve" => {
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
                if command["op"] == "resolve" {
                    let client = api.create_client();
                    let key = command["flag"].as_str().unwrap();
                    let result = match command["kind"].as_str().unwrap() {
                        "boolean" => client
                            .get_bool_details(key, Some(&context), None)
                            .await
                            .map(|v| v.into_value()),
                        "string" => client
                            .get_string_details(key, Some(&context), None)
                            .await
                            .map(|v| v.into_value()),
                        "integer" => client
                            .get_int_details(key, Some(&context), None)
                            .await
                            .map(|v| v.into_value()),
                        "float" => client
                            .get_float_details(key, Some(&context), None)
                            .await
                            .map(|v| v.into_value()),
                        "object" => client
                            .get_struct_details::<open_feature::StructValue>(
                                key,
                                Some(&context),
                                None,
                            )
                            .await
                            .map(|v| v.into_value()),
                        _ => panic!("Unknown kind"),
                    };
                    let result = match result {
                        Ok(details) => {
                            json!({"value": of_value(details.value), "error":"", "variant":details.variant, "named_type": details.flag_metadata.values.get("partly.result_type").map(|v| match v { open_feature::FlagMetadataValue::String(s) => s.as_str(), _ => panic!("Expected type name") })})
                        }
                        Err(error) => json!({"error":error.code.to_string()}),
                    };
                    println!("{result}");
                    io::stdout().flush()?;
                    continue;
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

fn of_value(value: open_feature::Value) -> Value {
    match value {
        open_feature::Value::Bool(v) => json!(v),
        open_feature::Value::Int(v) => json!(v),
        open_feature::Value::Float(v) => json!(v),
        open_feature::Value::String(v) => json!(v),
        open_feature::Value::Struct(v) => Value::Object(
            v.fields
                .into_iter()
                .map(|(k, v)| (k, of_value(v)))
                .collect(),
        ),
        open_feature::Value::Array(v) => Value::Array(v.into_iter().map(of_value).collect()),
    }
}
