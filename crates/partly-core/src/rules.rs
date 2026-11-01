// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use crate::{Condition, Operator, models::RawCondition, numbers::compare_numbers};
use regex::Regex;
use regex_syntax::ast::{Ast, Flag as RegexFlag};
use serde_json::Value;
use std::cmp::Ordering;

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

pub(crate) fn compile_regex(pattern: &str) -> Result<Regex, String> {
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

impl Condition {
    pub(crate) fn matches(&self, actual: &Value) -> bool {
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
