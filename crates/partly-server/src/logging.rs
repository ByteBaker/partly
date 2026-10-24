// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use axum::{
    body::Body,
    extract::{ConnectInfo, MatchedPath},
    http::{Method, Request, StatusCode},
    middleware::Next,
    response::Response,
};
use std::{net::SocketAddr, time::Instant};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::{filter::Targets, prelude::*};

pub fn init(level: LevelFilter) {
    tracing_subscriber::registry()
        // Dependency debug logs can include URLs and headers; enable only Partly's events.
        .with(Targets::new().with_target("partly", level))
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_ansi(false)
                .with_writer(std::io::stderr),
        )
        .init();
}

#[derive(Clone)]
pub struct Failure(pub String);

pub async fn request(request: Request<Body>, next: Next) -> Response {
    let started = Instant::now();
    let method = match *request.method() {
        Method::GET => "GET",
        Method::HEAD => "HEAD",
        Method::POST => "POST",
        Method::PUT => "PUT",
        Method::DELETE => "DELETE",
        Method::PATCH => "PATCH",
        Method::OPTIONS => "OPTIONS",
        Method::CONNECT => "CONNECT",
        Method::TRACE => "TRACE",
        _ => "OTHER",
    };
    let route = request
        .extensions()
        .get::<MatchedPath>()
        .map(|path| path.as_str().to_owned())
        .unwrap_or_else(|| "<unmatched>".into());
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|peer| peer.0.ip().to_canonical().to_string())
        .unwrap_or_else(|| "unknown".into());
    let response = next.run(request).await;
    let code = response.status();
    let status = code.as_u16();
    let elapsed_ms = started.elapsed().as_millis() as u64;
    if code.is_server_error() {
        let diagnostic = response
            .extensions()
            .get::<Failure>()
            .map(|failure| failure.0.as_str())
            .unwrap_or("Internal request failure");
        tracing::error!(target: "partly", method, route, peer, status, elapsed_ms, diagnostic, "Request failed");
    } else if code == StatusCode::UNAUTHORIZED && route == "/api/login" {
        tracing::warn!(target: "partly", method, route, peer, status, elapsed_ms, "Sign-in rejected");
    } else if matches!(
        code,
        StatusCode::FORBIDDEN | StatusCode::MISDIRECTED_REQUEST
    ) {
        tracing::warn!(target: "partly", method, route, peer, status, elapsed_ms, "Request rejected");
    } else if code.is_success()
        && route.starts_with("/api/")
        && route != "/api/usage"
        && !matches!(method, "GET" | "HEAD" | "OPTIONS")
    {
        tracing::info!(target: "partly", method, route, peer, status, elapsed_ms, "Request completed");
    } else {
        tracing::debug!(target: "partly", method, route, peer, status, elapsed_ms, "Request completed");
    }
    response
}
