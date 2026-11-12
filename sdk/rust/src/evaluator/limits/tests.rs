// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::evaluator::{Policy, rules::compile_regex};
use serde_json::json;

#[test]
fn short_patterns_cannot_expand_into_large_compiled_regexes() {
    assert!(compile_regex(r"\w{50}").unwrap_err().contains("256 KiB"));
    assert!(compile_regex("a{100000}").is_err());
    assert!(
        compile_regex(&"x".repeat(MAX_REGEX_BYTES + 1))
            .unwrap_err()
            .contains("1024")
    );
    for pattern in [r"^Pro[0-9]+$", r"^[^@]+@example\.com$", r"\w+", r"\p{L}+"] {
        assert!(compile_regex(pattern).is_ok(), "{pattern}");
    }
}

#[test]
fn flag_budget_is_checked_before_compilation() {
    let invalid = json!({"k":"id","op":"regex","v":"["});
    let flag =
        json!({"mode":"custom","action":"enable","groups":[vec![invalid; MAX_POLICY_REGEXES + 1]]});
    let error = serde_json::from_value::<Flag>(flag)
        .unwrap_err()
        .to_string();
    assert!(error.contains("64 regex"), "{error}");
}

#[test]
fn policy_budget_is_checked_across_flags_before_compilation() {
    for typed in [false, true] {
        let flags: BTreeMap<_, _> = (0..=MAX_POLICY_REGEXES).map(|i| {
            let rollout = json!({"mode":"custom","action":"enable","groups":[[{"k":"id","op":"regex","v":"["}]]});
            let flag = if typed { json!({"mode":"typed","result_type":"string","selected":"yes","other":"no","rollout":rollout}) } else { rollout };
            (format!("flag_{i}"), flag)
        }).collect();
        let policy = json!({"schema_version":2,"application_id":"app_test","project":"default","environment":"development","revision":1,"flags":flags});
        let error = Policy::from_json(&serde_json::to_vec(&policy).unwrap()).unwrap_err();
        assert!(error.contains("64 regex"), "{error}");
    }
}

#[test]
fn aggregate_budget_also_applies_to_constructed_policies() {
    let condition: Condition =
        serde_json::from_value(json!({"k":"id","op":"regex","v":"^user"})).unwrap();
    let flag = Flag::Custom {
        action: crate::evaluator::MatchAction::Enable,
        groups: vec![vec![condition]],
    };
    let mut policy = Policy {
        schema_version: 1,
        project: "default".into(),
        environment: "development".into(),
        application_id: "app_test".into(),
        revision: 1,
        types: BTreeMap::new(),
        flags: (0..MAX_POLICY_REGEXES)
            .map(|i| (format!("f_{i}"), flag.clone()))
            .collect(),
    };
    policy.validate().unwrap();
    policy.flags.insert("too_many".into(), flag);
    assert!(policy.validate().unwrap_err().contains("64 regex"));
}

#[test]
fn non_regex_conditions_remain_limited_by_policy_bytes() {
    let flag = json!({"mode":"custom","action":"enable","groups":[vec![json!({"k":"plan","op":"matches","v":"pro"}); 129]]});
    let flags: BTreeMap<_, _> = (0..9).map(|i| (format!("f_{i}"), flag.clone())).collect();
    let policy = json!({"schema_version":1,"application_id":"app_test","project":"default","environment":"development","revision":1,"flags":flags});
    Policy::from_json(&serde_json::to_vec(&policy).unwrap()).unwrap();
}
