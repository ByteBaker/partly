// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::{super::models::KeyName, session::session};
use crate::store::{self, Role};
use crate::{ApiError, api, state::App};
use axum::http::StatusCode;
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};

/// Create an additional named API Key.
#[utoipa::path(
    post,
    path = "/api/environments/{env}/apps/{id}/keys",
    tag = "Applications",
    request_body = KeyName,
    params(("env" = String, Path), ("id" = String, Path)),
    security(("session" = [])),
    responses(
        (status = 200, description = "Success", body = store::SdkKey),
        (status = 404, description = "Requested item not found", body = api::ErrorBody),
        (status = 413, description = "Request exceeds 16 KiB", body = String, content_type = "text/plain"),
        (status = 415, description = "Expected JSON content type", body = String, content_type = "text/plain"),
        (status = 422, description = "JSON does not match the request schema", body = String, content_type = "text/plain"),
        (status = 400, description = "Malformed JSON or invalid request", content((api::ErrorBody = "application/json"), (String = "text/plain")))
    )
)]
pub(crate) async fn create_key(
    State(app): State<App>,
    headers: HeaderMap,
    Path((env, id)): Path<(String, String)>,
    Json(input): Json<KeyName>,
) -> Result<Json<store::SdkKey>, ApiError> {
    app.run(move |s| {
        let user = s.authorize(&session(&headers))?;
        user.require(&env, Role::Manager)?;
        Ok(Json(s.create_key(
            &user.username,
            &env,
            &id,
            &input.name,
        )?))
    })
    .await
}

/// Rename an active API Key.
#[utoipa::path(
    put,
    path = "/api/keys/{id}",
    tag = "Applications",
    request_body = KeyName,
    params(("id" = String, Path)),
    security(("session" = [])),
    responses(
        (status = 204, description = "No content"),
        (status = 404, description = "Requested item not found", body = api::ErrorBody),
        (status = 413, description = "Request exceeds 16 KiB", body = String, content_type = "text/plain"),
        (status = 415, description = "Expected JSON content type", body = String, content_type = "text/plain"),
        (status = 422, description = "JSON does not match the request schema", body = String, content_type = "text/plain"),
        (status = 400, description = "Malformed JSON or invalid request", content((api::ErrorBody = "application/json"), (String = "text/plain")))
    )
)]
pub(crate) async fn rename_key(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<KeyName>,
) -> Result<StatusCode, ApiError> {
    app.run(move |s| {
        let user = s.authorize(&session(&headers))?;
        user.require(&s.key_environment(&id)?, Role::Manager)?;
        s.change_key(&user.username, &id, Some(&input.name))?;
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}

/// Revoke an API Key; connected SDKs clear policy when they next receive confirmed revocation.
#[utoipa::path(
    delete,
    path = "/api/keys/{id}",
    tag = "Applications",
    params(("id" = String, Path)),
    security(("session" = [])),
    responses(
        (status = 204, description = "No content"),
        (status = 404, description = "Requested item not found", body = api::ErrorBody)
    )
)]
pub(crate) async fn revoke_key(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    app.run(move |s| {
        let user = s.authorize(&session(&headers))?;
        user.require(&s.key_environment(&id)?, Role::Manager)?;
        s.change_key(&user.username, &id, None)?;
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}
