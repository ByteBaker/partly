// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use serde_json::{Number, value::RawValue};
use std::{cmp::Ordering, collections::BTreeMap};

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

// Preserve integer precision: casting both sides to f64 makes adjacent large IDs compare equal.
pub(crate) fn compare_numbers(left: &Number, right: &Number) -> Ordering {
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
