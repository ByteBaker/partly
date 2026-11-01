// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::session::session;
use crate::{ApiError, api, state::App};
use crate::{
    auth,
    store::{self, UserInput},
};
use axum::http::StatusCode;
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};

/// List accounts and permissions.
#[utoipa::path(
    get,
    path = "/api/users",
    tag = "Users",
    security(("session" = [])),
    responses(
        (status = 200, description = "Success", body = api::Users)
    )
)]
pub(crate) async fn users(
    State(app): State<App>,
    headers: HeaderMap,
) -> Result<Json<api::Users>, ApiError> {
    app.run(move |s| {
        s.authorize(&session(&headers))?.require_admin()?;
        Ok(Json(api::Users { users: s.users()? }))
    })
    .await
}

pub(crate) async fn write_user(
    app: App,
    headers: HeaderMap,
    id: Option<i64>,
    mut input: UserInput,
) -> Result<Json<store::User>, ApiError> {
    let token = session(&headers);
    let check = token.clone();
    app.run(move |s| s.authorize(&check)?.require_admin())
        .await?;
    input.validate()?;
    let hash = match input.password.take() {
        Some(password) => Some(app.auth.run(move || auth::hash_password(&password)).await?),
        None => None,
    };
    app.run(move |s| {
        let actor = s.authorize(&token)?;
        actor.require_admin()?;
        Ok(Json(s.save_user(
            &actor.username,
            id,
            &input,
            hash.as_deref(),
        )?))
    })
    .await
}

/// Create an account; password is required.
#[utoipa::path(
    post,
    path = "/api/users",
    tag = "Users",
    request_body = api::CreateUserInput,
    security(("session" = [])),
    responses(
        (status = 201, description = "Success", body = store::User),
        (status = 429, description = "Password workers busy or login throttled", body = api::ErrorBody, headers(("Retry-After" = u32, description = "60 seconds"))),
        (status = 409, description = "Conflict, duplicate, or stale flag impact token", body = api::ErrorBody),
        (status = 413, description = "Request exceeds 16 KiB", body = String, content_type = "text/plain"),
        (status = 415, description = "Expected JSON content type", body = String, content_type = "text/plain"),
        (status = 422, description = "JSON does not match the request schema", body = String, content_type = "text/plain"),
        (status = 400, description = "Malformed JSON or invalid request", content((api::ErrorBody = "application/json"), (String = "text/plain")))
    )
)]
pub(crate) async fn create_user(
    State(app): State<App>,
    headers: HeaderMap,
    Json(input): Json<UserInput>,
) -> Result<(StatusCode, Json<store::User>), ApiError> {
    Ok((
        StatusCode::CREATED,
        write_user(app, headers, None, input).await?,
    ))
}

/// Update an account; password is optional; changes invalidate its sessions.
#[utoipa::path(
    put,
    path = "/api/users/{id}",
    tag = "Users",
    request_body = UserInput,
    params(("id" = i64, Path)),
    security(("session" = [])),
    responses(
        (status = 200, description = "Success", body = store::User),
        (status = 429, description = "Password workers busy or login throttled", body = api::ErrorBody, headers(("Retry-After" = u32, description = "60 seconds"))),
        (status = 409, description = "Conflict, duplicate, or stale flag impact token", body = api::ErrorBody),
        (status = 404, description = "Requested item not found", body = api::ErrorBody),
        (status = 413, description = "Request exceeds 16 KiB", body = String, content_type = "text/plain"),
        (status = 415, description = "Expected JSON content type", body = String, content_type = "text/plain"),
        (status = 422, description = "JSON does not match the request schema", body = String, content_type = "text/plain"),
        (status = 400, description = "Malformed JSON or invalid request", content((api::ErrorBody = "application/json"), (String = "text/plain")))
    )
)]
pub(crate) async fn update_user(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Json(input): Json<UserInput>,
) -> Result<Json<store::User>, ApiError> {
    write_user(app, headers, Some(id), input).await
}
