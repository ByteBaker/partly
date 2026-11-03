// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::{ApiError, api, state::App, store};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use sha2::{Digest, Sha256};

fn bearer_key(headers: &HeaderMap) -> Result<String, ApiError> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_owned)
        .ok_or_else(ApiError::unauthorized)
}

/// Report distinct known flag checks; ignore names no longer assigned; receipt time is stored without changing policy or audits.
#[utoipa::path(
    post,
    path = "/api/usage",
    tag = "SDK",
    request_body = store::UsageInput,
    security(("api_key" = [])),
    responses(
        (status = 204, description = "No content"),
        (status = 401, description = "Invalid key/scope, or confirmed revocation with code key_revoked and X-Partly-Error header", body = api::ErrorBody, headers(("X-Partly-Error" = String, description = "key_revoked only for confirmed revocation"))),
        (status = 413, description = "Request exceeds 16 KiB", body = api::ErrorBody)
    )
)]
pub(crate) async fn report_usage(
    State(app): State<App>,
    headers: HeaderMap,
    input: Result<Json<store::UsageInput>, axum::extract::rejection::JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let key = bearer_key(&headers)?;
    let Json(input) = input.map_err(|error| {
        ApiError(
            if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
                StatusCode::PAYLOAD_TOO_LARGE
            } else {
                StatusCode::BAD_REQUEST
            },
            "Supply a JSON object containing only a flags array".into(),
        )
    })?;
    app.run(move |s| {
        let env = headers
            .get("x-partly-environment")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let application = headers
            .get("x-partly-application")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        s.record_usage(&key, env, application, input)?;
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}
/// Download the application policy; authenticate before checking If-None-Match.
///
/// A 200 contains the complete selected policy and a quoted SHA-256 content ETag.
/// Identical saves and changes outside the application selection keep its ETag unchanged.
/// The revision increases only when that application's selected policy changes.
/// A confirmed key_revoked response clears SDK policy even if the ETag matched;
/// ordinary errors preserve the last accepted snapshot.
#[utoipa::path(
    get,
    path = "/api/policy",
    tag = "SDK",
    params(("If-None-Match" = Option<String>, Header, description = "Previously accepted quoted ETag; weak tags, lists and wildcard are supported")),
    security(("api_key" = [])),
    responses(
        (status = 200, description = "Success", body = partly::evaluator::Policy, headers(("ETag" = String, description = "Quoted SHA-256 content hash"))),
        (status = 304, description = "Unchanged policy; no body", headers(("ETag" = String))),
        (status = 401, description = "Invalid key/scope, or confirmed revocation with code key_revoked and X-Partly-Error header", body = api::ErrorBody, headers(("X-Partly-Error" = String, description = "key_revoked only for confirmed revocation")))
    )
)]
pub(crate) async fn policy(
    State(app): State<App>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let key = bearer_key(&headers)?;
    app.run(move |s| {
        let env = headers
            .get("x-partly-environment")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let application = headers
            .get("x-partly-application")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let body = serde_json::to_vec(&s.app_policy(&key, env, application)?)
            .map_err(|_| ApiError::internal("Cannot encode policy"))?;
        let etag = format!("\"{:x}\"", Sha256::digest(&body));
        let matches = headers
            .get(header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| {
                v.split(',')
                    .any(|v| v.trim().trim_start_matches("W/") == etag || v.trim() == "*")
            });
        let mut response = if matches {
            StatusCode::NOT_MODIFIED.into_response()
        } else {
            ([(header::CONTENT_TYPE, "application/json")], body).into_response()
        };
        response
            .headers_mut()
            .insert(header::ETAG, HeaderValue::from_str(&etag).unwrap());
        Ok(response)
    })
    .await
}
