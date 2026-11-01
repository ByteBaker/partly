// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use crate::{Condition, Operator};
use serde_json::{json, Value};
use utoipa::{
    openapi::{RefOr, Schema},
    PartialSchema, ToSchema,
};

// The operator determines v's type; an unconstrained JSON value would hide invalid rules.
impl PartialSchema for Condition {
    fn schema() -> RefOr<Schema> {
        let operators =
            serde_json::to_value(Operator::schema()).expect("Operator schema serializes");
        let branches: Vec<_> = operators["enum"].as_array().expect("String operators").iter().map(|op| {
            let operator: Operator = serde_json::from_value(op.clone()).expect("Known operator");
            json!({
                "type": "object", "additionalProperties": false,
                "required": ["k", "op", "v"],
                "properties": {
                    "k": {"type": "string", "minLength": 1, "description": "Nonblank flat context key without control characters."},
                    "op": {"type": "string", "enum": [op]},
                    "v": {"type": operator.expected_type()}
                }
            })
        }).collect();
        serde_json::from_value(json!({"oneOf": branches})).expect("Condition schema is valid")
    }
}

impl ToSchema for Condition {}

pub fn groups() -> RefOr<Schema> {
    let schema: Value = json!({
        "type": "array", "minItems": 1,
        "description": "OR between groups; AND within each group. Both levels must be nonempty.",
        "items": {"type": "array", "minItems": 1, "items": {"$ref": "#/components/schemas/Condition"}}
    });
    serde_json::from_value(schema).expect("Rule groups schema is valid")
}
