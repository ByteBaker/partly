// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

pub(crate) mod error;
mod handlers;
mod models;
mod openapi;
#[cfg(test)]
mod tests;

use crate::state::App;
use axum::Router;
pub(crate) use models::{
    Applications, Audits, Environments, ErrorBody, Impact, LoginResult, RevocationCode, Status,
    Usage, Users,
};
pub(crate) use openapi::CreateUserInput;
use utoipa::{OpenApi as _, openapi::OpenApi};
use utoipa_axum::{router::OpenApiRouter, routes};

pub fn router() -> (Router<App>, OpenApi) {
    let (router, mut document) = OpenApiRouter::with_openapi(openapi::Contract::openapi())
        .routes(routes!(handlers::session::status))
        .routes(routes!(handlers::session::setup))
        .routes(routes!(handlers::session::login))
        .routes(routes!(handlers::session::logout))
        .routes(routes!(handlers::session::environments))
        .routes(routes!(
            handlers::users::users,
            handlers::users::create_user
        ))
        .routes(routes!(handlers::users::update_user))
        .routes(routes!(
            handlers::flags::flags,
            handlers::flags::create_flag
        ))
        .routes(routes!(
            handlers::flags::save_flag,
            handlers::flags::delete_flag
        ))
        .routes(routes!(handlers::flags::flag_impact))
        .routes(routes!(
            handlers::types::types,
            handlers::types::create_type
        ))
        .routes(routes!(handlers::types::delete_type))
        .routes(routes!(
            handlers::applications::applications,
            handlers::applications::create_application
        ))
        .routes(routes!(handlers::applications::save_application))
        .routes(routes!(handlers::keys::create_key))
        .routes(routes!(handlers::audits::audits))
        .routes(routes!(handlers::flags::flag_usage))
        .routes(routes!(
            handlers::keys::rename_key,
            handlers::keys::revoke_key
        ))
        .routes(routes!(handlers::sdk::policy))
        .routes(routes!(handlers::sdk::report_usage))
        .split_for_parts();
    openapi::finish(&mut document);
    (router, document)
}
