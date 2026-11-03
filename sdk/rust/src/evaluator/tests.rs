// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::evaluator::{numbers::compare_numbers, rules::compile_regex};
use serde_json::Value;
use serde_json::json;
use std::cmp::Ordering;

fn policy(flag: Value) -> Policy {
    Policy::from_json(&serde_json::to_vec(&json!({"schema_version":1,"application_id":"app_test","project":"default","environment":"development","revision":1,"flags":{"checkout":flag}})).unwrap()).unwrap()
}

#[test]
fn policy_size_limit_applies_before_parsing() {
    let mut bytes = serde_json::to_vec(&policy(json!({"mode":"enabled"}))).unwrap();
    bytes.resize(MAX_POLICY_BYTES, b' ');
    assert!(Policy::from_json(&bytes).is_ok());
    bytes.push(b' ');
    assert_eq!(
        Policy::from_json(&bytes).unwrap_err(),
        "Policy exceeds the 1 MiB POC limit"
    );
}
fn context(value: Value) -> Context {
    serde_json::from_value(value).unwrap()
}
fn custom(groups: Value, action: &str) -> Policy {
    policy(json!({"mode":"custom","action":action,"groups":groups}))
}

#[test]
fn boundaries_and_monotonic_rollout() {
    let policies: Vec<_> = [0, 25, 50, 100]
        .map(|percentage| policy(json!({"mode":"percentage","percentage":percentage})))
        .into();
    let mut selected = 0;
    for i in 0..10_000 {
        let context = context(json!({"id":format!("user-{i}")}));
        assert_eq!(
            policies[0].evaluate("checkout", &context),
            Ok(Decision::Disabled)
        );
        assert_eq!(
            policies[3].evaluate("checkout", &context),
            Ok(Decision::Enabled)
        );
        if policies[1].evaluate("checkout", &context) == Ok(Decision::Enabled) {
            selected += 1;
            assert_eq!(
                policies[2].evaluate("checkout", &context),
                Ok(Decision::Enabled)
            );
        }
    }
    assert!((2200..2800).contains(&selected));
    for (mode, decision) in [
        ("enabled", Decision::Enabled),
        ("disabled", Decision::Disabled),
    ] {
        assert_eq!(
            policy(json!({"mode":mode})).evaluate("checkout", &Context::new()),
            Ok(decision)
        );
    }
}

#[test]
fn operators_match_exact_types_and_case() {
    let cases = [
        ("le", json!(18), json!(18.0), json!(19)),
        ("lt", json!(18), json!(-0.5), json!(18)),
        ("ge", json!(18), json!(18), json!(17.9)),
        ("gt", json!(18), json!(18.1), json!(18)),
        ("eq", json!(18), json!(18.0), json!(18.1)),
        ("prefix", json!("Pro"), json!("Product"), json!("product")),
        ("suffix", json!(".com"), json!("a.com"), json!("a.COM")),
        ("contains", json!("Pro"), json!("aProZ"), json!("aproZ")),
        ("matches", json!("Pro"), json!("Pro"), json!("ProPlus")),
        (
            "regex",
            json!("^Pro[0-9]+$"),
            json!("Pro42"),
            json!("pro42"),
        ),
        ("is", json!(false), json!(false), json!(true)),
    ];
    for (op, expected, yes, no) in cases {
        let policy = custom(json!([[{"k":"x","op":op,"v":expected}]]), "enable");
        assert_eq!(
            policy.evaluate("checkout", &context(json!({"x":yes}))),
            Ok(Decision::Enabled),
            "{op}"
        );
        assert_eq!(
            policy.evaluate("checkout", &context(json!({"x":no}))),
            Ok(Decision::Disabled),
            "{op}"
        );
    }
}

#[test]
fn groups_actions_and_errors_do_not_short_circuit() {
    let groups = json!([[{"k":"plan","op":"matches","v":"pro"},{"k":"age","op":"ge","v":18}], [{"k":"staff","op":"is","v":true}]]);
    for (action, yes, no) in [
        ("enable", Decision::Enabled, Decision::Disabled),
        ("disable", Decision::Disabled, Decision::Enabled),
    ] {
        let policy = custom(groups.clone(), action);
        assert_eq!(
            policy.evaluate(
                "checkout",
                &context(json!({"plan":"basic","age":10,"staff":true}))
            ),
            Ok(yes)
        );
        assert_eq!(
            policy.evaluate(
                "checkout",
                &context(json!({"plan":"pro","age":18,"staff":false}))
            ),
            Ok(yes)
        );
        assert_eq!(
            policy.evaluate(
                "checkout",
                &context(json!({"plan":"pro","age":17,"staff":false}))
            ),
            Ok(no)
        );
        assert!(matches!(
            policy.evaluate("checkout", &context(json!({"plan":"pro","age":18}))),
            Err(EvaluationError::InvalidContext { .. })
        ));
        assert!(matches!(
            policy.evaluate(
                "checkout",
                &context(json!({"plan":"basic","age":"private","staff":true}))
            ),
            Err(EvaluationError::ContextType {
                op: Some("ge"),
                expected_type: "number",
                actual_type: "string",
                ..
            })
        ));
    }
    let policy = custom(json!([[{"k":"age","op":"ge","v":18}]]), "enable");
    let error = policy
        .evaluate("checkout", &context(json!({"age":"private"})))
        .unwrap_err();
    assert!(!error.to_string().contains("private"));
    assert!(matches!(
        policy.evaluate("missing", &Context::new()),
        Err(EvaluationError::FlagNotFound(_))
    ));
}

#[test]
fn invalid_policies_and_regex_flags_are_rejected() {
    for flag in [
        json!({"mode":"percentage","percentage":101}),
        json!({"mode":"enabled","percentage":10}),
        json!({"mode":"custom","action":"enable","groups":[]}),
        json!({"mode":"custom","action":"enable","groups":[[]]}),
    ] {
        assert!(
            serde_json::from_value::<Flag>(flag)
                .and_then(|flag| flag.validate().map_err(serde::de::Error::custom))
                .is_err()
        );
    }
    for condition in [
        json!({"k":"x","op":"eq","v":true}),
        json!({"k":"x","op":"is","v":1}),
        json!({"k":"x","op":"bool","v":true}),
        json!({"k":"x","op":"matches","v":null}),
        json!({"k":"x","op":"ge","v":1,"extra":1}),
    ] {
        assert!(serde_json::from_value::<Condition>(condition).is_err());
    }
    for pattern in [
        "[",
        "(?i)pro",
        "(?i:pro)",
        "(?x) ( ?i : pro )",
        "(?=pro)",
        r"(a)\1",
    ] {
        assert!(compile_regex(pattern).is_err(), "{pattern}");
    }
    for pattern in [
        "(?-i)pro",
        r"\(\?i\)",
        "[?i]",
        "(?x) pro # (?i) in a comment\n",
    ] {
        assert!(compile_regex(pattern).is_ok(), "{pattern}");
    }
    let flag = json!({"mode":"custom","action":"enable","groups":[[{"k":"plan","op":"regex","v":"^Pro$"}]]});
    let decoded: Flag = serde_json::from_value(flag.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), flag);
}

#[test]
fn numeric_comparison_preserves_large_integers() {
    for (a, b, order) in [
        (
            json!(9007199254740993_u64),
            json!(9007199254740992.0),
            Ordering::Greater,
        ),
        (json!(u64::MAX), json!(u64::MAX as f64), Ordering::Less),
        (json!(i64::MIN), json!(i64::MIN as f64), Ordering::Equal),
        (json!(0), json!(-0.5), Ordering::Greater),
        (json!(-1), json!(-1.5), Ordering::Greater),
        (json!(18), json!(18.0), Ordering::Equal),
        (json!(u64::MAX), json!(i64::MAX), Ordering::Greater),
    ] {
        assert_eq!(
            compare_numbers(a.as_number().unwrap(), b.as_number().unwrap()),
            order
        );
        assert_eq!(
            compare_numbers(b.as_number().unwrap(), a.as_number().unwrap()),
            order.reverse()
        );
    }
}

#[test]
fn json_floats_preserve_their_numeric_value() {
    let policy = Policy::from_json(
        br#"{"schema_version":1,"application_id":"app_test","project":"default","environment":"development","revision":1,
        "flags":{"checkout":{"mode":"custom","action":"enable","groups":[[
            {"k":"value","op":"eq","v":1.0000000000000001e18}
        ]]}}}"#,
    )
    .unwrap();
    assert_eq!(
        policy.evaluate(
            "checkout",
            &context(json!({"value": 1_000_000_000_000_000_128_u64}))
        ),
        Ok(Decision::Enabled)
    );
}

#[test]
fn numeric_literals_outside_supported_range_are_rejected_without_rounding() {
    for literal in [
        "-9223372036854775809",
        "18446744073709551616",
        "1e400",
        "-1e400",
    ] {
        let json = format!(
            r#"{{"schema_version":1,"application_id":"app_test","project":"default","environment":"development","revision":1,"flags":{{"checkout":{{"mode":"custom","action":"enable","groups":[[{{"k":"x","op":"eq","v":{literal}}}]]}}}}}}"#
        );
        assert!(Policy::from_json(json.as_bytes()).is_err(), "{literal}");
    }
    for literal in [
        "-9223372036854775808",
        "18446744073709551615",
        "1e300",
        "-1e300",
    ] {
        let json = format!(
            r#"{{"schema_version":1,"application_id":"app_test","project":"default","environment":"development","revision":1,"flags":{{"checkout":{{"mode":"custom","action":"enable","groups":[[{{"k":"x","op":"eq","v":{literal}}}]]}}}}}}"#
        );
        assert!(Policy::from_json(json.as_bytes()).is_ok(), "{literal}");
    }
    assert!(validate_json_number_limits(br#"{"\u0076": [-9223372036854775809]}"#).is_err());
    assert!(
        validate_json_number_limits(
            br#"{"v": ["-9223372036854775809", "escaped \"18446744073709551616\""]}"#
        )
        .is_ok()
    );
    let nested = format!("{}0{}", "[".repeat(129), "]".repeat(129));
    assert!(validate_json_number_limits(nested.as_bytes()).is_err());
}

#[test]
fn percentage_requires_valid_identity_and_preserves_hash() {
    let policy = policy(json!({"mode":"percentage","percentage":0}));
    for id in ["", " ", &"x".repeat(257)] {
        assert!(matches!(
            policy.evaluate("checkout", &context(json!({"id":id}))),
            Err(EvaluationError::InvalidContext { .. })
        ));
    }
    assert!(
        matches!(policy.evaluate("checkout", &context(json!({"id":42}))),Err(EvaluationError::ContextType { k, op:None, .. }) if k == "id")
    );
    assert_ne!(bucket("ab", "c"), bucket("a", "bc"));
    assert_eq!(bucket("checkout", "user-42"), 9426);
    assert_eq!(bucket("new_checkout", "user-123"), 4195);
    assert_eq!(bucket("checkout", "用户-1"), 8114);
}
