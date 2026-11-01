// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::support::{driver::*, *};
use serde_json::{Value, json};
use std::{
    fs,
    time::{Duration, Instant},
};

mod lifecycle;
mod native;
mod openfeature;
mod refresh;

use native::contract;
use openfeature::openfeature;
use refresh::{bad_refresh, scope};

macro_rules! language_tests {
    ($($language:ident),+) => {$ (
        mod $language {
            use super::*;
            #[tokio::test] async fn native_contract() -> Result { contract(stringify!($language)).await }
            #[tokio::test] async fn application_scope_and_background_revocation() -> Result { scope(stringify!($language)).await }
            #[tokio::test] async fn bad_refresh_preserves_snapshot_and_etag() -> Result { bad_refresh(stringify!($language)).await }
            #[tokio::test] async fn openfeature_contract() -> Result { openfeature(stringify!($language)).await }
        }
    )+};
}
language_tests!(python, rust, go, node);

fn type_error(flag: &str, key: &str, op: Value, expected: &str, actual: &str) -> Value {
    json!({"error":"context_type","flag_key":flag,"k":key,"op":op,"expected_type":expected,"actual_type":actual})
}

pub async fn wait_evaluate(
    driver: &mut Driver,
    flag: &str,
    context: Value,
    expected: Value,
) -> Result {
    let deadline = Instant::now() + Duration::from_secs(5);
    while driver.evaluate(flag, context.clone()).await? != expected {
        assert!(
            Instant::now() < deadline,
            "Background refresh did not reach {expected}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Ok(())
}
