// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::session::session;
use crate::{
    ApiError,
    api::{ErrorBody, models::NamedTypeInput},
    state::App,
    store::Role,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use partly_core::TypeDefinition;
use std::collections::BTreeMap;

/// List named result types in this environment.
#[utoipa::path(get, path = "/api/environments/{env}/types", tag = "Types",
    params(("env" = String, Path)), security(("session" = [])),
    responses((status = 200, description = "Named types", body = BTreeMap<String, TypeDefinition>)))]
pub(crate) async fn types(
    State(app): State<App>,
    headers: HeaderMap,
    Path(env): Path<String>,
) -> Result<Json<BTreeMap<String, TypeDefinition>>, ApiError> {
    app.run(move |s| {
        s.authorize(&session(&headers))?
            .require(&env, Role::Viewer)?;
        Ok(Json(s.result_types(&env)?))
    })
    .await
}

/// Create an immutable named enum or record. Names remain reserved after deletion.
#[utoipa::path(post, path = "/api/environments/{env}/types", tag = "Types",
    params(("env" = String, Path)), security(("session" = [])), request_body = NamedTypeInput,
    responses((status = 201, description = "Created type", body = NamedTypeInput),
        (status = 409, description = "Name already used", body = ErrorBody)))]
pub(crate) async fn create_type(
    State(app): State<App>,
    headers: HeaderMap,
    Path(env): Path<String>,
    Json(input): Json<NamedTypeInput>,
) -> Result<(StatusCode, Json<NamedTypeInput>), ApiError> {
    app.run(move |s| {
        let user = s.authorize(&session(&headers))?;
        user.require(&env, Role::Manager)?;
        s.create_type(&user.username, &env, &input.name, &input.definition)?;
        Ok((StatusCode::CREATED, Json(input)))
    })
    .await
}

/// Delete an unused type. Its name stays reserved to prevent changes in meaning.
#[utoipa::path(delete, path = "/api/environments/{env}/types/{name}", tag = "Types",
    params(("env" = String, Path), ("name" = String, Path)), security(("session" = [])),
    responses((status = 204, description = "Deleted"), (status = 404, description = "Unknown type", body = ErrorBody),
        (status = 409, description = "Type still referenced", body = ErrorBody)))]
pub(crate) async fn delete_type(
    State(app): State<App>,
    headers: HeaderMap,
    Path((env, name)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    app.run(move |s| {
        let user = s.authorize(&session(&headers))?;
        user.require(&env, Role::Manager)?;
        s.delete_type(&user.username, &env, &name)?;
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}
