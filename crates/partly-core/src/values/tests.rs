// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use crate::{Context, Decision, EvaluationError, Policy};
use serde_json::{Value, json};

fn document() -> Value {
    json!({"schema_version":2,"project":"default","environment":"development","application_id":"app_test","revision":1,
    "types":{"Layout":{"kind":"enum","values":["compact","wide"]},"Settings":{"kind":"record","fields":{"layout":"Layout","retries":"integer","help":"boolean"}}},
    "flags":{
        "old":{"mode":"enabled"},
        "text":{"mode":"typed","result_type":"string","selected":"hello","other":"bye","rollout":{"mode":"enabled"}},
        "config":{"mode":"typed","result_type":"Settings","selected":{"layout":"wide","retries":3,"help":true},"other":{"layout":"compact","retries":1,"help":false},"rollout":{"mode":"percentage","percentage":50}}
    }})
}
fn parse(value: &Value) -> Result<Policy, String> {
    Policy::from_json(&serde_json::to_vec(value).unwrap())
}

#[test]
fn typed_results_preserve_rollouts_and_distinguish_type_errors() {
    let p = parse(&document()).unwrap();
    assert_eq!(
        p.evaluate("old", &Context::new()).unwrap(),
        Decision::Enabled
    );
    assert_eq!(
        p.resolve("text", &Context::new(), "string", None)
            .unwrap()
            .value,
        "hello"
    );
    assert!(matches!(
        p.evaluate("text", &Context::new()),
        Err(EvaluationError::ResultType { .. })
    ));
    assert!(matches!(
        p.resolve("missing", &Context::new(), "object", None),
        Err(EvaluationError::FlagNotFound(_))
    ));
    let context = Context::from([("id".into(), json!("user-42"))]);
    let mut result = p
        .resolve("config", &context, "named", Some("Settings"))
        .unwrap();
    assert_eq!(result.named_type.as_deref(), Some("Settings"));
    assert_eq!(
        result.value["help"],
        crate::bucket("config", "user-42") < 5000
    );
    assert!(matches!(
        p.resolve("config", &context, "named", Some("OtherSettings")),
        Err(EvaluationError::NamedType { .. })
    ));
    result.value["layout"] = json!("changed");
    assert_ne!(
        p.resolve("config", &context, "object", None).unwrap().value["layout"],
        "changed"
    );
    assert!(matches!(
        p.resolve("config", &Context::new(), "object", None),
        Err(EvaluationError::InvalidContext { .. })
    ));
}

#[test]
fn schemas_and_both_branches_are_validated_before_acceptance() {
    let initial = document();
    for bad in [
        json!({"layout":"wide","retries":3}),
        json!({"layout":"invalid","retries":3,"help":true}),
        json!({"layout":"wide","retries":3,"help":"true"}),
        json!({"layout":"wide","retries":3,"help":true,"extra":0}),
    ] {
        let mut value = initial.clone();
        value["flags"]["config"]["other"] = bad;
        assert!(parse(&value).is_err());
    }
    let mut value = initial.clone();
    value["types"]["Settings"]["fields"]["nested"] = json!("Settings");
    assert!(parse(&value).is_err());
    let mut value = initial.clone();
    value["types"]["Layout"]["values"] = json!(["wide", "wide"]);
    assert!(parse(&value).is_err());
    let mut value = initial.clone();
    value["schema_version"] = json!(1);
    assert!(parse(&value).is_err());
    let mut value = initial;
    value["flags"]["text"]["rollout"] = value["flags"]["config"].clone();
    assert!(parse(&value).is_err());
}

#[test]
fn numeric_kinds_are_explicit_and_bounded() {
    for (kind, value, valid) in [
        ("integer", json!(9_007_199_254_740_991_i64), true),
        ("integer", json!(-9_007_199_254_740_991_i64), true),
        ("integer", json!(9_007_199_254_740_992_i64), false),
        ("integer", json!(1.5), false),
        ("integer", json!(true), false),
        ("float", json!(3), true),
        ("float", json!("3"), false),
    ] {
        let mut doc = document();
        doc["flags"]["text"]["result_type"] = json!(kind);
        doc["flags"]["text"]["selected"] = value.clone();
        doc["flags"]["text"]["other"] = value;
        let parsed = parse(&doc);
        assert_eq!(parsed.is_ok(), valid);
        if let Ok(policy) = parsed {
            let resolution = policy
                .resolve("text", &Context::new(), "number", None)
                .unwrap();
            assert_eq!(resolution.kind, kind);
            assert!(
                policy
                    .resolve(
                        "text",
                        &Context::new(),
                        if kind == "integer" {
                            "float"
                        } else {
                            "integer"
                        },
                        None
                    )
                    .is_err()
            );
        }
    }
}
