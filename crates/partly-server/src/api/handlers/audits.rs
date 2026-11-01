// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::{super::models::AuditQuery, session::session};
use crate::store::Role;
use crate::{ApiError, api, state::App};
use axum::extract::Query;
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};

/// Read at most 100 audit entries, newest first; use the last ID as before for the next page.
#[utoipa::path(
    get,
    path = "/api/environments/{env}/audits",
    tag = "Audits",
    params(("env" = String, Path), AuditQuery),
    security(("session" = [])),
    responses(
        (status = 200, description = "Success", body = api::Audits),
        (status = 400, description = "Malformed parameters or invalid request", content((api::ErrorBody = "application/json"), (String = "text/plain")))
    )
)]
pub(crate) async fn audits(
    State(app): State<App>,
    headers: HeaderMap,
    Path(env): Path<String>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<api::Audits>, ApiError> {
    app.run(move |s| {
        let user = s.authorize(&session(&headers))?;
        user.require(&env, Role::Viewer)?;
        Ok(Json(api::Audits {
            audits: s.audits(
                &env,
                query.before,
                query.action.as_deref(),
                query.application_id.as_deref(),
            )?,
        }))
    })
    .await
}
