// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

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
        assert!(
            std::ffi::CStr::from_ptr(result)
                .to_str()
                .unwrap()
                .contains("flag_not_found")
        );
        partly_string_free(result);
        partly_snapshot_free(snapshot);
    }
}
