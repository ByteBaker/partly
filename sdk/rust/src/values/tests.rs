// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use crate::evaluator::Policy;
use crate::{Client, Context, Error, EvaluationError, models::Accepted};
use serde_json::json;
use std::sync::Arc;

#[test]
fn typed_native_getters_keep_values_owned_and_errors_structured() {
    let document = json!({"schema_version":2,"application_id":"app_test","project":"default","environment":"development","revision":1,
      "types":{"Settings":{"kind":"record","fields":{"count":"integer","rate":"float"}}},
      "flags":{
        "record":{"mode":"typed","result_type":"Settings","selected":{"count":3,"rate":3},"other":{"count":0,"rate":0},"rollout":{"mode":"enabled"}},
        "integer":{"mode":"typed","result_type":"integer","selected":3,"other":0,"rollout":{"mode":"enabled"}},
        "float":{"mode":"typed","result_type":"float","selected":3,"other":0,"rollout":{"mode":"enabled"}},
        "string":{"mode":"typed","result_type":"string","selected":"yes","other":"no","rollout":{"mode":"enabled"}}}});
    let client = Client::builder()
        .url("http://127.0.0.1:1")
        .environment("development")
        .application_id("app_test")
        .secret_key("key")
        .build()
        .unwrap();
    *client.inner.accepted.write().unwrap() = Some(Accepted {
        policy: Arc::new(Policy::from_json(&serde_json::to_vec(&document).unwrap()).unwrap()),
        etag: "one".into(),
    });
    let context = Context::new();
    assert_eq!(client.evaluate_integer("integer", &context).unwrap(), 3);
    assert_eq!(client.evaluate_float("float", &context).unwrap(), 3.0);
    assert_eq!(client.evaluate_string("string", &context).unwrap(), "yes");
    let mut record = client.evaluate_object("record", &context).unwrap();
    assert!(record["count"].is_i64());
    assert!(record["rate"].is_f64());
    record.insert("count".into(), json!(99));
    assert_eq!(
        client
            .evaluate_named("record", &context, "Settings")
            .unwrap()["count"],
        3
    );
    assert!(
        matches!(client.evaluate_named("record", &context, "Other"), Err(Error::Evaluation(EvaluationError::NamedType { expected_type, actual_type, .. })) if expected_type == "Other" && actual_type == "Settings")
    );
    assert!(matches!(
        client.evaluate_integer("float", &context),
        Err(Error::Evaluation(EvaluationError::ResultType { .. }))
    ));
}
