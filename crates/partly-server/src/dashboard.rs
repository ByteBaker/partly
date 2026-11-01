// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::{ApiError, api, state::App};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use std::sync::Arc;

pub(crate) fn router() -> Router<App> {
    let (api_router, contract) = api::router();
    let contract = Arc::new(contract);
    Router::new()
        .merge(api_router)
        .route(
            "/openapi.json",
            get(move || async move { Json(contract.as_ref().clone()) }),
        )
        .route("/", get(dashboard))
        .route("/environments/{env}/{screen}", get(environment_dashboard))
        .route(
            "/logo.svg",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "image/svg+xml")],
                    include_str!("../../../docs/images/logo.svg"),
                )
            }),
        )
        .route(
            "/app.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../dashboard/app.js"),
                )
            }),
        )
        .route(
            "/dropdown.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../dashboard/dropdown.js"),
                )
            }),
        )
        .route(
            "/style.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    include_str!("../dashboard/style.css"),
                )
            }),
        )
        .route(
            "/docs/{example}",
            get(|Path(example): Path<String>| async move {
                let source = match example.as_str() {
                    "python.py" => include_str!("../../../examples/python/quickstart.py"),
                    "rust.rs" => include_str!("../../../examples/rust/quickstart.rs"),
                    "go.go" => include_str!("../../../examples/go/main.go"),
                    "node.js" => include_str!("../../../examples/node/quickstart.js"),
                    _ => return Err(StatusCode::NOT_FOUND),
                };
                Ok((
                    [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                    source,
                ))
            }),
        )
}

async fn dashboard() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        include_str!("../dashboard/index.html"),
    )
}

async fn environment_dashboard(
    State(app): State<App>,
    Path((env, screen)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    if !matches!(
        screen.as_str(),
        "flags" | "apps" | "audits" | "docs" | "users"
    ) {
        return Err(ApiError::not_found("Unknown screen"));
    }
    app.run(move |s| s.environment(&env)).await?;
    Ok(dashboard().await.into_response())
}
