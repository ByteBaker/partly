// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

//! JSON boundary shared by the Go and Node.js bindings. Policies remain parsed in Rust.

use partly_core::{validate_json_number_limits, Context, Decision, EvaluationError, Policy};
use serde::Deserialize;
use serde_json::json;
use std::{
    ffi::{c_char, CString},
    panic::{catch_unwind, AssertUnwindSafe},
    ptr, slice,
};

pub struct Snapshot(Policy);

impl Snapshot {
    pub fn new(bytes: &[u8]) -> Result<Self, String> {
        Policy::from_json(bytes).map(Self)
    }
    pub fn contains_flag(&self, key: &str) -> bool {
        self.0.flags.contains_key(key)
    }
    pub fn scope(&self) -> String {
        json!([self.0.project, self.0.environment, self.0.application_id]).to_string()
    }
    pub fn evaluate(&self, bytes: &[u8]) -> String {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Request {
            flag: String,
            context: Context,
        }
        let invalid = || {
            json!({"error": "invalid_context", "message": "Supply a flat context with supported numbers"}).to_string()
        };
        if validate_json_number_limits(bytes).is_err() {
            return invalid();
        }
        let Ok(request) = serde_json::from_slice::<Request>(bytes) else {
            return invalid();
        };
        match self.0.evaluate(&request.flag, &request.context) {
            Ok(decision) => json!({"decision": match decision {
                Decision::Enabled => "enabled", Decision::Disabled => "disabled",
            }}),
            Err(error) => {
                let message = error.to_string();
                match error {
                    EvaluationError::FlagNotFound(flag) => json!({
                        "error": "flag_not_found", "message": message, "flag_key": flag,
                    }),
                    EvaluationError::InvalidContext { flag_key, k, .. } => json!({
                        "error": "invalid_context", "message": message, "flag_key": flag_key, "k": k,
                    }),
                    EvaluationError::ContextType { flag_key, k, op, expected_type, actual_type } => json!({
                        "error": "context_type", "message": message, "flag_key": flag_key, "k": k,
                        "op": op, "expected_type": expected_type, "actual_type": actual_type,
                    }),
                }
            }
        }.to_string()
    }
}

/// # Safety
/// `snapshot` must be live, and `data` must point to `len` readable bytes.
/// Returns 1 for a known UTF-8 flag key and 0 otherwise; allocates no returned memory.
#[no_mangle]
pub unsafe extern "C" fn partly_snapshot_has_flag(
    snapshot: *const Snapshot,
    data: *const u8,
    len: usize,
) -> std::ffi::c_int {
    std::str::from_utf8(slice::from_raw_parts(data, len))
        .is_ok_and(|key| (*snapshot).contains_flag(key))
        .into()
}

fn c_string(value: String) -> *mut c_char {
    // Serialized JSON never contains literal NULs.
    CString::new(value).unwrap().into_raw()
}

/// # Safety
/// `data` must point to `len` readable bytes, and `result` to a writable pointer.
/// The caller owns both returned allocations and must use the corresponding free functions.
#[no_mangle]
pub unsafe extern "C" fn partly_snapshot_new(
    data: *const u8,
    len: usize,
    result: *mut *mut c_char,
) -> *mut Snapshot {
    let parsed = catch_unwind(|| Snapshot::new(slice::from_raw_parts(data, len)));
    match parsed {
        Ok(Ok(snapshot)) => {
            *result = c_string(snapshot.scope());
            Box::into_raw(Box::new(snapshot))
        }
        other => {
            let message = match other {
                Ok(Err(message)) => message,
                _ => "Native validation failed".into(),
            };
            *result = c_string(json!({"error": "invalid_policy", "message": message}).to_string());
            ptr::null_mut()
        }
    }
}

/// # Safety
/// `snapshot` must be live, and `data` must point to `len` readable bytes.
/// The caller must free the returned string with `partly_string_free`.
#[no_mangle]
pub unsafe extern "C" fn partly_snapshot_evaluate(
    snapshot: *const Snapshot,
    data: *const u8,
    len: usize,
) -> *mut c_char {
    c_string(
        catch_unwind(AssertUnwindSafe(|| {
            (*snapshot).evaluate(slice::from_raw_parts(data, len))
        }))
        .unwrap_or_else(|_| {
            json!({"error":"invalid_context", "message":"Native evaluation failed"}).to_string()
        }),
    )
}

/// # Safety
/// Pass a pointer returned by `partly_snapshot_new`, exactly once, after all readers finish.
#[no_mangle]
pub unsafe extern "C" fn partly_snapshot_free(snapshot: *mut Snapshot) {
    drop(Box::from_raw(snapshot));
}

/// # Safety
/// Pass a string returned by this library, exactly once.
#[no_mangle]
pub unsafe extern "C" fn partly_string_free(value: *mut c_char) {
    drop(CString::from_raw(value));
}

#[cfg(test)]
mod tests {
    use super::*;
    const POLICY: &[u8] =
        br#"{"schema_version":1,"application_id":"app_test","project":"default","environment":"development","revision":1,
        "flags":{"flag":{"mode":"percentage","percentage":50}}}"#;
    #[test]
    fn boundary_preserves_errors_and_rejects_rounded_integers() {
        let snapshot = Snapshot::new(POLICY).unwrap();
        for (context, code) in [
            (r#"{"id":42}"#, "context_type"),
            ("{}", "invalid_context"),
            (
                r#"{"id":"a","age":18446744073709551616}"#,
                "invalid_context",
            ),
        ] {
            let result: serde_json::Value = serde_json::from_str(
                &snapshot.evaluate(format!(r#"{{"flag":"flag","context":{context}}}"#).as_bytes()),
            )
            .unwrap();
            assert_eq!(result["error"], code);
        }
    }
    #[test]
    fn ffi_allocations_round_trip() {
        unsafe {
            let mut result = ptr::null_mut();
            let snapshot = partly_snapshot_new(POLICY.as_ptr(), POLICY.len(), &mut result);
            assert!(!snapshot.is_null());
            partly_string_free(result);
            for (key, expected) in [
                (b"flag".as_slice(), 1),
                (b"missing".as_slice(), 0),
                (b"".as_slice(), 0),
                (b"\xff".as_slice(), 0),
            ] {
                assert_eq!(
                    partly_snapshot_has_flag(snapshot, key.as_ptr(), key.len()),
                    expected
                );
            }
            let request = br#"{"flag":"missing","context":{}}"#;
            let result = partly_snapshot_evaluate(snapshot, request.as_ptr(), request.len());
            assert!(std::ffi::CStr::from_ptr(result)
                .to_str()
                .unwrap()
                .contains("flag_not_found"));
            partly_string_free(result);
            partly_snapshot_free(snapshot);
        }
    }
}
