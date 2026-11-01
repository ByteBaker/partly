// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

#[test]
fn committed_contract_matches_registered_handlers() {
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("../../../../docs/openapi.json")).unwrap();
    let actual = serde_json::to_value(super::router().1).unwrap();
    assert_eq!(
        actual, expected,
        "Run task api:export after changing the HTTP API"
    );
}
