// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::{api, logging};
use axum::{
    Json,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};

#[derive(Debug)]
pub(crate) struct ApiError(pub(crate) StatusCode, pub(crate) String);

impl ApiError {
    pub(crate) fn bad(message: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, message.into())
    }
    pub(crate) fn conflict(message: impl Into<String>) -> Self {
        Self(StatusCode::CONFLICT, message.into())
    }
    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self(StatusCode::NOT_FOUND, message.into())
    }
    pub(crate) fn internal(message: impl Into<String>) -> Self {
        Self(StatusCode::INTERNAL_SERVER_ERROR, message.into())
    }
    pub(crate) fn unauthorized() -> Self {
        Self(
            StatusCode::UNAUTHORIZED,
            "Authentication required or credentials invalid".into(),
        )
    }
    pub(crate) fn forbidden() -> Self {
        Self(
            StatusCode::FORBIDDEN,
            "Your account does not have permission for this action".into(),
        )
    }
    pub(crate) fn throttled() -> Self {
        Self(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many sign-in attempts or password workers busy. Retry after 60 seconds.".into(),
        )
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(error: rusqlite::Error) -> Self {
        match error.sqlite_error() {
            Some(code) => Self::internal(format!(
                "Database operation failed (SQLite code {})",
                code.extended_code
            )),
            None => Self::internal("Database operation failed"),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let revoked = self.0 == StatusCode::UNAUTHORIZED && self.1 == "key_revoked";
        let diagnostic = self
            .0
            .is_server_error()
            .then(|| logging::Failure(self.1.clone()));
        let message = if self.0.is_server_error() {
            "Storage operation failed. Check the server terminal.".to_owned()
        } else {
            self.1
        };
        let mut response = (
            self.0,
            Json(api::ErrorBody {
                error: if revoked {
                    "API Key revoked".into()
                } else {
                    message
                },
                code: revoked.then_some(api::RevocationCode::KeyRevoked),
            }),
        )
            .into_response();
        if revoked {
            response
                .headers_mut()
                .insert("x-partly-error", HeaderValue::from_static("key_revoked"));
        }
        if self.0 == StatusCode::TOO_MANY_REQUESTS {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
        }
        if let Some(diagnostic) = diagnostic {
            response.extensions_mut().insert(diagnostic);
        }
        response
    }
}
