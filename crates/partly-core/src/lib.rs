// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use regex::Regex;
use regex_syntax::ast::{Ast, Flag as RegexFlag};
use serde::{Deserialize, Serialize};
use serde_json::{value::RawValue, Number, Value};
use sha2::{Digest, Sha256};
use std::{cmp::Ordering, collections::BTreeMap};
use thiserror::Error;

pub type Context = BTreeMap<String, Value>;
pub const MAX_POLICY_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum Flag {
    Enabled {},
    Disabled {},
    Percentage {
        percentage: u8,
    },
    Custom {
        action: MatchAction,
        groups: Vec<Vec<Condition>>,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchAction {
    Enable,
    Disable,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operator {
    Le,
    Lt,
    Ge,
    Gt,
    Eq,
    Prefix,
    Suffix,
    Contains,
    Matches,
    Regex,
    Is,
}

impl Operator {
    pub fn name(self) -> &'static str {
        match self {
            Self::Le => "le",
            Self::Lt => "lt",
            Self::Ge => "ge",
            Self::Gt => "gt",
            Self::Eq => "eq",
            Self::Prefix => "prefix",
            Self::Suffix => "suffix",
            Self::Contains => "contains",
            Self::Matches => "matches",
            Self::Regex => "regex",
            Self::Is => "is",
        }
    }
    pub fn expected_type(self) -> &'static str {
        match self {
            Self::Le | Self::Lt | Self::Ge | Self::Gt | Self::Eq => "number",
            Self::Is => "boolean",
            _ => "string",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "RawCondition")]
pub struct Condition {
    k: String,
    op: Operator,
    v: Value,
    #[serde(skip)]
    regex: Option<Regex>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCondition {
    k: String,
    op: Operator,
    v: Value,
}

impl TryFrom<RawCondition> for Condition {
    type Error = String;
    fn try_from(raw: RawCondition) -> Result<Self, Self::Error> {
        if raw.k.trim().is_empty() || raw.k.chars().any(char::is_control) {
            return Err(
                "Condition k must be a nonblank context key without control characters".into(),
            );
        }
        if value_type(&raw.v) != raw.op.expected_type() {
            return Err(format!(
                "Operator {} expects a {} value",
                raw.op.name(),
                raw.op.expected_type()
            ));
        }
        let regex = if matches!(raw.op, Operator::Regex) {
            Some(compile_regex(raw.v.as_str().unwrap())?)
        } else {
            None
        };
        Ok(Self {
            k: raw.k,
            op: raw.op,
            v: raw.v,
            regex,
        })
    }
}

fn compile_regex(pattern: &str) -> Result<Regex, String> {
    let ast = regex_syntax::ast::parse::Parser::new()
        .parse(pattern)
        .map_err(|_| "Invalid Rust regex pattern".to_string())?;
    let mut pending = vec![&ast];
    while let Some(node) = pending.pop() {
        let flags = match node {
            Ast::Flags(set) => Some(&set.flags),
            Ast::Group(group) => {
                pending.push(&group.ast);
                group.flags()
            }
            Ast::Repetition(repetition) => {
                pending.push(&repetition.ast);
                None
            }
            Ast::Alternation(alternation) => {
                pending.extend(&alternation.asts);
                None
            }
            Ast::Concat(concat) => {
                pending.extend(&concat.asts);
                None
            }
            _ => None,
        };
        if flags.is_some_and(|flags| flags.flag_state(RegexFlag::CaseInsensitive) == Some(true)) {
            return Err(
                "Regex must be case-sensitive; inline case-insensitive flags are not allowed"
                    .into(),
            );
        }
    }
    Regex::new(pattern).map_err(|_| "Invalid or oversized Rust regex pattern".into())
}

impl Flag {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::Percentage { percentage } if *percentage > 100 => {
                Err("Percentage must be 0–100".into())
            }
            Self::Custom { groups, .. }
                if groups.is_empty() || groups.iter().any(Vec::is_empty) =>
            {
                Err("Custom rules need at least one group and one condition in each group".into())
            }
            // Conditions are immutable and validated, including compiled regex, on deserialization.
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub schema_version: u32,
    pub project: String,
    pub environment: String,
    pub revision: u64,
    pub flags: BTreeMap<String, Flag>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Enabled,
    Disabled,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EvaluationError {
    #[error("Flag '{0}' is not defined in the loaded policy")]
    FlagNotFound(String),
    #[error("Invalid context for flag '{flag_key}', key '{k}': {reason}")]
    InvalidContext {
        flag_key: String,
        k: String,
        reason: &'static str,
    },
    #[error("Context type mismatch for flag '{flag_key}', key '{k}': expected {expected_type}, got {actual_type}")]
    ContextType {
        flag_key: String,
        k: String,
        op: Option<&'static str>,
        expected_type: &'static str,
        actual_type: &'static str,
    },
}

pub fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Bool(_) => "boolean",
        Value::Null => "null",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

// Check original literals before accepting parsed numbers: serde_json rounds overflowing integers.
pub fn validate_json_number_limits(json: &[u8]) -> Result<(), String> {
    let root: &RawValue = serde_json::from_slice(json).map_err(|error| error.to_string())?;
    let mut pending = vec![(root, 0)];
    while let Some((value, depth)) = pending.pop() {
        if depth >= 128 {
            return Err("JSON nesting exceeds the supported depth".into());
        }
        let text = value.get();
        match text.as_bytes()[0] {
            b'{' => {
                let fields: BTreeMap<String, &RawValue> =
                    serde_json::from_str(text).map_err(|error| error.to_string())?;
                // Serde's private marker must never turn a supplied object into a scalar value.
                if fields.contains_key("$serde_json::private::RawValue") {
                    return Err("Reserved JSON object fields are not supported".into());
                }
                pending.extend(fields.into_values().map(|value| (value, depth + 1)));
            }
            b'[' => {
                let values: Vec<&RawValue> =
                    serde_json::from_str(text).map_err(|error| error.to_string())?;
                pending.extend(values.into_iter().map(|value| (value, depth + 1)));
            }
            b'-' | b'0'..=b'9'
                if !text.contains(['.', 'e', 'E'])
                    && text.parse::<i64>().is_err()
                    && text.parse::<u64>().is_err() =>
            {
                return Err("Integer values must fit signed or unsigned 64-bit values".into());
            }
            _ => {}
        }
    }
    Ok(())
}

pub fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 64
        && key
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b"_-".contains(&c))
}

impl Policy {
    pub fn from_json(json: &[u8]) -> Result<Self, String> {
        if json.len() > MAX_POLICY_BYTES {
            return Err("Policy exceeds the 1 MiB POC limit".into());
        }
        let policy: Self = serde_json::from_slice(json).map_err(|e| e.to_string())?;
        policy.validate()?;
        validate_json_number_limits(json)?;
        Ok(policy)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("Unsupported policy schema_version".into());
        }
        if !valid_key(&self.project) || !valid_key(&self.environment) {
            return Err("Invalid policy project or environment".into());
        }
        for (key, flag) in &self.flags {
            if !valid_key(key) {
                return Err("Invalid flag key".into());
            }
            flag.validate()?;
        }
        Ok(())
    }

    pub fn evaluate(&self, key: &str, context: &Context) -> Result<Decision, EvaluationError> {
        let flag = self
            .flags
            .get(key)
            .ok_or_else(|| EvaluationError::FlagNotFound(key.to_owned()))?;
        // Validate every required field before matching: an early match must not hide an error.
        match flag {
            Flag::Percentage { .. } => {
                let id = required(context, key, "id", None, "string")?
                    .as_str()
                    .unwrap();
                if id.trim().is_empty() || id.len() > 256 {
                    return Err(EvaluationError::InvalidContext {
                        flag_key: key.into(),
                        k: "id".into(),
                        reason: "id must contain 1–256 UTF-8 bytes and cannot be blank",
                    });
                }
            }
            Flag::Custom { groups, .. } => {
                for condition in groups.iter().flatten() {
                    required(
                        context,
                        key,
                        &condition.k,
                        Some(condition.op.name()),
                        condition.op.expected_type(),
                    )?;
                }
            }
            _ => {}
        }
        for (k, v) in context {
            if !matches!(v, Value::String(_) | Value::Number(_) | Value::Bool(_)) {
                return Err(EvaluationError::ContextType {
                    flag_key: key.into(),
                    k: k.clone(),
                    op: None,
                    expected_type: "string, number, or boolean",
                    actual_type: value_type(v),
                });
            }
        }
        let enabled = match flag {
            Flag::Enabled {} => true,
            Flag::Disabled {} => false,
            Flag::Percentage { percentage } => {
                bucket(key, context["id"].as_str().unwrap()) < u32::from(*percentage) * 100
            }
            Flag::Custom { action, groups } => {
                let matched = groups.iter().any(|group| {
                    group
                        .iter()
                        .all(|condition| condition.matches(&context[&condition.k]))
                });
                matched == matches!(action, MatchAction::Enable)
            }
        };
        Ok(if enabled {
            Decision::Enabled
        } else {
            Decision::Disabled
        })
    }
}

fn required<'a>(
    context: &'a Context,
    flag_key: &str,
    k: &str,
    op: Option<&'static str>,
    expected_type: &'static str,
) -> Result<&'a Value, EvaluationError> {
    let value = context
        .get(k)
        .ok_or_else(|| EvaluationError::InvalidContext {
            flag_key: flag_key.into(),
            k: k.into(),
            reason: "required key is missing",
        })?;
    let actual_type = value_type(value);
    if actual_type != expected_type {
        return Err(EvaluationError::ContextType {
            flag_key: flag_key.into(),
            k: k.into(),
            op,
            expected_type,
            actual_type,
        });
    }
    Ok(value)
}

impl Condition {
    fn matches(&self, actual: &Value) -> bool {
        match self.op {
            Operator::Le | Operator::Lt | Operator::Ge | Operator::Gt | Operator::Eq => {
                let order =
                    compare_numbers(actual.as_number().unwrap(), self.v.as_number().unwrap());
                match self.op {
                    Operator::Le => order != Ordering::Greater,
                    Operator::Lt => order == Ordering::Less,
                    Operator::Ge => order != Ordering::Less,
                    Operator::Gt => order == Ordering::Greater,
                    _ => order == Ordering::Equal,
                }
            }
            Operator::Is => actual.as_bool() == self.v.as_bool(),
            _ => {
                let actual = actual.as_str().unwrap();
                let expected = self.v.as_str().unwrap();
                match self.op {
                    Operator::Prefix => actual.starts_with(expected),
                    Operator::Suffix => actual.ends_with(expected),
                    Operator::Contains => actual.contains(expected),
                    Operator::Matches => actual == expected,
                    Operator::Regex => self.regex.as_ref().unwrap().is_match(actual),
                    _ => unreachable!(),
                }
            }
        }
    }
}

// Preserve integer precision: casting both sides to f64 makes adjacent large IDs compare equal.
fn compare_numbers(left: &Number, right: &Number) -> Ordering {
    fn integer(number: &Number) -> Option<i128> {
        number
            .as_i64()
            .map(i128::from)
            .or_else(|| number.as_u64().map(i128::from))
    }
    fn integer_float(integer: i128, float: f64) -> Ordering {
        if float >= i128::MAX as f64 {
            return Ordering::Less;
        }
        if float <= i128::MIN as f64 {
            return Ordering::Greater;
        }
        match integer.cmp(&(float as i128)) {
            Ordering::Equal => 0.0_f64.partial_cmp(&float.fract()).unwrap(),
            order => order,
        }
    }
    match (integer(left), integer(right)) {
        (Some(left), Some(right)) => left.cmp(&right),
        (Some(left), None) => integer_float(left, right.as_f64().unwrap()),
        (None, Some(right)) => integer_float(right, left.as_f64().unwrap()).reverse(),
        (None, None) => left
            .as_f64()
            .unwrap()
            .partial_cmp(&right.as_f64().unwrap())
            .unwrap(),
    }
}

// Versioned, length-prefixed input avoids ambiguous concatenation and process-specific hashes.
pub fn bucket(key: &str, user_id: &str) -> u32 {
    let mut hash = Sha256::new();
    hash.update(b"partly:v1:");
    for value in [key, user_id] {
        hash.update((value.len() as u32).to_be_bytes());
        hash.update(value.as_bytes());
    }
    let digest = hash.finalize();
    u32::from_be_bytes(digest[..4].try_into().unwrap()) % 10_000
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn policy(flag: Value) -> Policy {
        Policy::from_json(&serde_json::to_vec(&json!({"schema_version":1,"project":"default","environment":"development","revision":1,"flags":{"checkout":flag}})).unwrap()).unwrap()
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
            assert!(serde_json::from_value::<Flag>(flag)
                .and_then(|flag| flag.validate().map_err(serde::de::Error::custom))
                .is_err());
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
            br#"{"schema_version":1,"project":"default","environment":"development","revision":1,
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
                r#"{{"schema_version":1,"project":"default","environment":"development","revision":1,"flags":{{"checkout":{{"mode":"custom","action":"enable","groups":[[{{"k":"x","op":"eq","v":{literal}}}]]}}}}}}"#
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
                r#"{{"schema_version":1,"project":"default","environment":"development","revision":1,"flags":{{"checkout":{{"mode":"custom","action":"enable","groups":[[{{"k":"x","op":"eq","v":{literal}}}]]}}}}}}"#
            );
            assert!(Policy::from_json(json.as_bytes()).is_ok(), "{literal}");
        }
        assert!(validate_json_number_limits(br#"{"\u0076": [-9223372036854775809]}"#).is_err());
        assert!(validate_json_number_limits(
            br#"{"v": ["-9223372036854775809", "escaped \"18446744073709551616\""]}"#
        )
        .is_ok());
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
}
