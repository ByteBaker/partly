// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

mod api;
mod auth;
mod cli;
mod dashboard;
mod logging;
mod server;
mod state;
mod store;
mod transport;

use api::error::ApiError;

#[tokio::main]
async fn main() {
    cli::run().await;
}
