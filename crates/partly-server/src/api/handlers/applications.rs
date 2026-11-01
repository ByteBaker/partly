// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::{super::models::ApplicationInput, session::session};
use crate::store::{self, Role};
use crate::{ApiError, api, state::App};
use axum::http::StatusCode;
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};

/// List applications and keys; secrets are null for viewers/editors and revoked keys.
#[utoipa::path(
    get,
    path = "/api/environments/{env}/apps",
    tag = "Applications",
    params(("env" = String, Path)),
    security(("session" = [])),
    responses(
        (status = 200, description = "Success", body = api::Applications)
    )
)]
pub(crate) async fn applications(
    State(app): State<App>,
    headers: HeaderMap,
    Path(env): Path<String>,
) -> Result<Json<api::Applications>, ApiError> {
    app.run(move |s| {
        let user = s.authorize(&session(&headers))?;
        user.require(&env, Role::Viewer)?;
        let mut apps = s.applications(&env)?;
        if !user.allows(&env, Role::Manager) {
            for app in &mut apps {
                app.hide_secrets();
            }
        }
        Ok(Json(api::Applications { applications: apps }))
    })
    .await
}

/// Create an application with selected flags and its first named API Key.
#[utoipa::path(
    post,
    path = "/api/environments/{env}/apps",
    tag = "Applications",
    request_body = ApplicationInput,
    params(("env" = String, Path)),
    security(("session" = [])),
    responses(
        (status = 201, description = "Success", body = store::Application),
        (status = 409, description = "Conflict, duplicate, or stale flag impact token", body = api::ErrorBody),
        (status = 413, description = "Request exceeds 16 KiB", body = String, content_type = "text/plain"),
        (status = 415, description = "Expected JSON content type", body = String, content_type = "text/plain"),
        (status = 422, description = "JSON does not match the request schema", body = String, content_type = "text/plain"),
        (status = 400, description = "Malformed JSON or invalid request", content((api::ErrorBody = "application/json"), (String = "text/plain")))
    )
)]
pub(crate) async fn create_application(
    State(app): State<App>,
    headers: HeaderMap,
    Path(env): Path<String>,
    Json(input): Json<ApplicationInput>,
) -> Result<(StatusCode, Json<store::Application>), ApiError> {
    app.run(move |s| {
        let user = s.authorize(&session(&headers))?;
        user.require(&env, Role::Manager)?;
        Ok((
            StatusCode::CREATED,
            Json(s.save_application(&user.username, &env, None, &input.name, &input.flags)?),
        ))
    })
    .await
}

/// Update application name and selected flags; keep the permanent ID.
#[utoipa::path(
    put,
    path = "/api/environments/{env}/apps/{id}",
    tag = "Applications",
    request_body = ApplicationInput,
    params(("env" = String, Path), ("id" = String, Path)),
    security(("session" = [])),
    responses(
        (status = 200, description = "Success", body = store::Application),
        (status = 409, description = "Conflict, duplicate, or stale flag impact token", body = api::ErrorBody),
        (status = 404, description = "Requested item not found", body = api::ErrorBody),
        (status = 413, description = "Request exceeds 16 KiB", body = String, content_type = "text/plain"),
        (status = 415, description = "Expected JSON content type", body = String, content_type = "text/plain"),
        (status = 422, description = "JSON does not match the request schema", body = String, content_type = "text/plain"),
        (status = 400, description = "Malformed JSON or invalid request", content((api::ErrorBody = "application/json"), (String = "text/plain")))
    )
)]
pub(crate) async fn save_application(
    State(app): State<App>,
    headers: HeaderMap,
    Path((env, id)): Path<(String, String)>,
    Json(input): Json<ApplicationInput>,
) -> Result<Json<store::Application>, ApiError> {
    app.run(move |s| {
        let user = s.authorize(&session(&headers))?;
        user.require(&env, Role::Manager)?;
        Ok(Json(s.save_application(
            &user.username,
            &env,
            Some(&id),
            &input.name,
            &input.flags,
        )?))
    })
    .await
}
