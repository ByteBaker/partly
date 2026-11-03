// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::{super::models::SavedFlag, session::session};
use crate::store::{Role, Store};
use crate::{ApiError, api, state::App};
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use axum::{
    body::Bytes,
    http::{StatusCode, header},
};
use partly::evaluator::Flag;
use serde::de::DeserializeOwned;
use serde_json::json;
use sha2::{Digest, Sha256};

/// List shared flags in an environment.
#[utoipa::path(
    get,
    path = "/api/environments/{env}/flags",
    tag = "Flags",
    params(("env" = String, Path)),
    security(("session" = [])),
    responses(
        (status = 200, description = "Success", body = std::collections::BTreeMap<String, Flag>)
    )
)]
pub(crate) async fn flags(
    State(app): State<App>,
    headers: HeaderMap,
    Path(env): Path<String>,
) -> Result<Json<std::collections::BTreeMap<String, Flag>>, ApiError> {
    app.run(move |s| {
        let user = s.authorize(&session(&headers))?;
        user.require(&env, Role::Viewer)?;
        Ok(Json(s.policy(&env)?.flags))
    })
    .await
}

fn parse_flag<T: DeserializeOwned>(headers: &HeaderMap, body: &[u8]) -> Result<T, ApiError> {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if content_type != "application/json"
        && !(content_type.starts_with("application/") && content_type.ends_with("+json"))
    {
        return Err(ApiError(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Expected an application/json body".into(),
        ));
    }
    let input = Json::<T>::from_bytes(body)
        .map(|Json(input)| input)
        .map_err(|error| ApiError(error.status(), error.body_text()))?;
    partly::evaluator::validate_json_number_limits(body)
        .map_err(|error| ApiError(StatusCode::UNPROCESSABLE_ENTITY, error))?;
    Ok(input)
}

/// Create a flag and return its saved state.
#[utoipa::path(
    post,
    path = "/api/environments/{env}/flags",
    tag = "Flags",
    request_body = SavedFlag,
    params(("env" = String, Path)),
    security(("session" = [])),
    responses(
        (status = 201, description = "Success", body = SavedFlag),
        (status = 409, description = "Conflict, duplicate, or stale flag impact token", body = api::ErrorBody),
        (status = 413, description = "Request exceeds 16 KiB", body = String, content_type = "text/plain"),
        (status = 415, description = "Expected JSON content type", body = api::ErrorBody),
        (status = 422, description = "Invalid JSON shape, condition or number", body = api::ErrorBody)
    )
)]
pub(crate) async fn create_flag(
    State(app): State<App>,
    headers: HeaderMap,
    Path(env): Path<String>,
    body: Bytes,
) -> Result<(StatusCode, Json<SavedFlag>), ApiError> {
    app.run(move |s| {
        let user = s.authorize(&session(&headers))?;
        user.require(&env, Role::Editor)?;
        // Rule validation compiles regex; do it after auth on the blocking worker.
        let input: SavedFlag = parse_flag(&headers, &body)?;
        s.save_flag(&user.username, &env, &input.key, &input.flag, true)?;
        Ok((StatusCode::CREATED, Json(input)))
    })
    .await
}

/// Save a flag after reviewing affected applications and confirming the exact environment/flag name.
#[utoipa::path(
    put,
    path = "/api/environments/{env}/flags/{key}",
    tag = "Flags",
    request_body = Flag,
    params(("env" = String, Path), ("key" = String, Path), ("X-Partly-Confirm" = String, Header, description = "Exact case-sensitive environment/flag_name"), ("X-Partly-Impact" = String, Header, description = "Token returned by the impact endpoint; stale previews return 409")),
    security(("session" = [])),
    responses(
        (status = 200, description = "Success", body = SavedFlag),
        (status = 409, description = "Conflict, duplicate, or stale flag impact token", body = api::ErrorBody),
        (status = 404, description = "Requested item not found", body = api::ErrorBody),
        (status = 413, description = "Request exceeds 16 KiB", body = String, content_type = "text/plain"),
        (status = 415, description = "Expected JSON content type", body = api::ErrorBody),
        (status = 422, description = "Invalid JSON shape, condition or number", body = api::ErrorBody)
    )
)]
pub(crate) async fn save_flag(
    State(app): State<App>,
    headers: HeaderMap,
    Path((env, key)): Path<(String, String)>,
    body: Bytes,
) -> Result<Json<SavedFlag>, ApiError> {
    app.run(move |s| {
        let user = s.authorize(&session(&headers))?;
        user.require(&env, Role::Editor)?;
        let flag: Flag = parse_flag(&headers, &body)?;
        if !partly::evaluator::valid_key(&key) {
            return Err(ApiError::bad("Invalid flag key"));
        }
        flag.validate().map_err(ApiError::bad)?;
        check_flag_confirmation(s, &headers, &env, &key)?;
        s.save_flag(&user.username, &env, &key, &flag, false)?;
        Ok(Json(SavedFlag { key, flag }))
    })
    .await
}

/// Delete a flag after reviewing affected applications and confirming the exact environment/flag name.
#[utoipa::path(
    delete,
    path = "/api/environments/{env}/flags/{key}",
    tag = "Flags",
    params(("env" = String, Path), ("key" = String, Path), ("X-Partly-Confirm" = String, Header, description = "Exact case-sensitive environment/flag_name"), ("X-Partly-Impact" = String, Header, description = "Token returned by the impact endpoint; stale previews return 409")),
    security(("session" = [])),
    responses(
        (status = 204, description = "No content"),
        (status = 409, description = "Conflict, duplicate, or stale flag impact token", body = api::ErrorBody),
        (status = 404, description = "Requested item not found", body = api::ErrorBody)
    )
)]
pub(crate) async fn delete_flag(
    State(app): State<App>,
    headers: HeaderMap,
    Path((env, key)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    app.run(move |s| {
        let user = s.authorize(&session(&headers))?;
        user.require(&env, Role::Editor)?;
        check_flag_confirmation(s, &headers, &env, &key)?;
        s.delete_flag(&user.username, &env, &key)?;
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}

fn flag_impact_value(s: &Store, env: &str, key: &str) -> Result<api::Impact, ApiError> {
    let apps = s.flag_impact(env, key)?;
    let policy = s.policy(env)?;
    let bytes = serde_json::to_vec(&json!({"flag": policy.flags.get(key), "applications": apps}))
        .map_err(|_| ApiError::internal("Cannot encode impact"))?;
    Ok(api::Impact {
        applications: apps,
        token: format!("{:x}", Sha256::digest(bytes)),
    })
}

fn check_flag_confirmation(
    s: &Store,
    headers: &HeaderMap,
    env: &str,
    key: &str,
) -> Result<(), ApiError> {
    let impact = flag_impact_value(s, env, key)?;
    if headers
        .get("x-partly-confirm")
        .and_then(|v| v.to_str().ok())
        != Some(format!("{env}/{key}").as_str())
    {
        return Err(ApiError::bad("Confirm the exact environment/flag name"));
    }
    if headers.get("x-partly-impact").and_then(|v| v.to_str().ok()) != Some(impact.token.as_str()) {
        return Err(ApiError::conflict(
            "The flag or its applications changed. Review the affected applications and confirm again.",
        ));
    }
    Ok(())
}

/// Read affected applications and the current confirmation token.
#[utoipa::path(
    get,
    path = "/api/environments/{env}/flags/{key}/impact",
    tag = "Flags",
    params(("env" = String, Path), ("key" = String, Path)),
    security(("session" = [])),
    responses(
        (status = 200, description = "Success", body = api::Impact),
        (status = 404, description = "Requested item not found", body = api::ErrorBody)
    )
)]
pub(crate) async fn flag_impact(
    State(app): State<App>,
    headers: HeaderMap,
    Path((env, key)): Path<(String, String)>,
) -> Result<Json<api::Impact>, ApiError> {
    app.run(move |s| {
        let user = s.authorize(&session(&headers))?;
        user.require(&env, Role::Viewer)?;
        Ok(Json(flag_impact_value(s, &env, &key)?))
    })
    .await
}
/// Read last reported usage per API Key and flag; missing reports do not prove a flag is unused.
#[utoipa::path(
    get,
    path = "/api/environments/{env}/usage",
    tag = "Flags",
    params(("env" = String, Path)),
    security(("session" = [])),
    responses(
        (status = 200, description = "Success", body = api::Usage)
    )
)]
pub(crate) async fn flag_usage(
    State(app): State<App>,
    headers: HeaderMap,
    Path(env): Path<String>,
) -> Result<Json<api::Usage>, ApiError> {
    app.run(move |s| {
        s.authorize(&session(&headers))?
            .require(&env, Role::Viewer)?;
        Ok(Json(api::Usage {
            usage: s.flag_usage(&env)?,
        }))
    })
    .await
}
