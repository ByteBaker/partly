// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::super::models::Credentials;
use crate::{ApiError, api, auth, state::App, store::Role};
use axum::{
    Json,
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use std::net::SocketAddr;

pub(super) fn session(headers: &HeaderMap) -> String {
    headers
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .find_map(|part| part.trim().strip_prefix("partly_session="))
        .unwrap_or("")
        .to_owned()
}

fn login_response(token: &str) -> Response {
    (
        [(
            header::SET_COOKIE,
            format!("partly_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200"),
        )],
        Json(api::LoginResult { ok: true }),
    )
        .into_response()
}
/// Read setup and session state.
#[utoipa::path(
    get,
    path = "/api/status",
    tag = "Session",
    responses(
        (status = 200, description = "Success", body = api::Status)
    )
)]
pub(crate) async fn status(
    State(app): State<App>,
    headers: HeaderMap,
) -> Result<Json<api::Status>, ApiError> {
    app.run(move |s| {
        Ok(Json(api::Status {
            setup_required: !s.ready(),
            authenticated: s.authenticated(&session(&headers)),
        }))
    })
    .await
}

/// Complete first setup and start an administrator session.
#[utoipa::path(
    post,
    path = "/api/setup",
    tag = "Session",
    request_body = Credentials,
    responses(
        (status = 200, description = "Success", body = api::LoginResult, headers(("Set-Cookie" = String, description = "HttpOnly; SameSite=Strict; Path=/; Secure on HTTPS. Max-Age=43200, or 0 for logout."))),
        (status = 429, description = "Password workers busy or login throttled", body = api::ErrorBody, headers(("Retry-After" = u32, description = "60 seconds"))),
        (status = 409, description = "Conflict, duplicate, or stale flag impact token", body = api::ErrorBody),
        (status = 413, description = "Request exceeds 16 KiB", body = String, content_type = "text/plain"),
        (status = 415, description = "Expected JSON content type", body = String, content_type = "text/plain"),
        (status = 422, description = "JSON does not match the request schema", body = String, content_type = "text/plain"),
        (status = 400, description = "Malformed JSON or invalid request", content((api::ErrorBody = "application/json"), (String = "text/plain")))
    )
)]
pub(crate) async fn setup(
    State(app): State<App>,
    Json(c): Json<Credentials>,
) -> Result<Response, ApiError> {
    app.run(|s| {
        if s.ready() {
            Err(ApiError::conflict("Setup is already complete"))
        } else {
            Ok(())
        }
    })
    .await?;
    if c.username.is_empty()
        || c.username.len() > 64
        || c.username.trim() != c.username
        || c.username.chars().any(char::is_control)
    {
        return Err(ApiError::bad(
            "Use a username of 1–64 bytes without control characters or surrounding whitespace",
        ));
    }
    auth::validate_password(&c.password)?;
    let hash = app
        .auth
        .run(move || auth::hash_password(&c.password))
        .await?;
    app.run(move |s| {
        s.setup(&c.username, &hash)?;
        Ok(login_response(&s.new_session(1)))
    })
    .await
}

/// Sign in; five attempts per peer IP per minute.
#[utoipa::path(
    post,
    path = "/api/login",
    tag = "Session",
    request_body = Credentials,
    responses(
        (status = 200, description = "Success", body = api::LoginResult, headers(("Set-Cookie" = String, description = "HttpOnly; SameSite=Strict; Path=/; Secure on HTTPS. Max-Age=43200, or 0 for logout."))),
        (status = 401, description = "Invalid credentials or disabled account", body = api::ErrorBody),
        (status = 429, description = "Password workers busy or login throttled", body = api::ErrorBody, headers(("Retry-After" = u32, description = "60 seconds"))),
        (status = 413, description = "Request exceeds 16 KiB", body = String, content_type = "text/plain"),
        (status = 415, description = "Expected JSON content type", body = String, content_type = "text/plain"),
        (status = 422, description = "JSON does not match the request schema", body = String, content_type = "text/plain"),
        (status = 400, description = "Malformed JSON or invalid request", content((api::ErrorBody = "application/json"), (String = "text/plain")))
    )
)]
pub(crate) async fn login(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(c): Json<Credentials>,
) -> Result<Response, ApiError> {
    app.auth.check_attempt(peer.ip())?;
    if c.password.len() > 1024 || c.username.len() > 64 {
        return Err(ApiError::unauthorized());
    }
    let username = c.username;
    let lookup = username.clone();
    let (id, hash, enabled) = app.run(move |s| s.credentials(&lookup)).await?;
    let checked_hash = hash.clone();
    let valid = app
        .auth
        .run(move || auth::verify_password(&c.password, &checked_hash))
        .await?;
    if !valid || !enabled {
        return Err(ApiError::unauthorized());
    }
    app.run(move |s| {
        // A reset or disable during password verification must invalidate the login too.
        let current = s.credentials(&username)?;
        if current != (id, hash, true) {
            return Err(ApiError::unauthorized());
        }
        Ok(login_response(&s.new_session(id)))
    })
    .await
}

/// Clear the current session.
#[utoipa::path(
    post,
    path = "/api/logout",
    tag = "Session",
    responses(
        (status = 204, description = "No content", headers(("Set-Cookie" = String, description = "HttpOnly; SameSite=Strict; Path=/; Secure on HTTPS. Max-Age=43200, or 0 for logout.")))
    )
)]
pub(crate) async fn logout(
    State(app): State<App>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    app.run(move |s| {
        s.logout(&session(&headers));
        Ok(())
    })
    .await?;
    Ok((
        [(
            header::SET_COOKIE,
            "partly_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
        )],
        StatusCode::NO_CONTENT,
    )
        .into_response())
}

/// List environments and the signed-in account permissions.
#[utoipa::path(
    get,
    path = "/api/environments",
    tag = "Session",
    security(("session" = [])),
    responses(
        (status = 200, description = "Success", body = api::Environments)
    )
)]
pub(crate) async fn environments(
    State(app): State<App>,
    headers: HeaderMap,
) -> Result<Json<api::Environments>, ApiError> {
    app.run(move |s| {
        let user = s.authorize(&session(&headers))?;
        let environments: Vec<_> = ["development", "production"]
            .into_iter()
            .filter(|env| user.allows(env, Role::Viewer))
            .collect();
        Ok(Json(api::Environments {
            project: "default",
            environments,
            user,
        }))
    })
    .await
}
