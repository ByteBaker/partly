// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::ApiError;
use axum::{
    body::Body,
    extract::State,
    http::{HeaderValue, Method, Request, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::net::IpAddr;

pub(crate) fn parse_hostname(value: &str) -> Result<String, String> {
    if value.len() > 253
        || value.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || !label.starts_with(|c: char| c.is_ascii_alphanumeric())
                || !label.ends_with(|c: char| c.is_ascii_alphanumeric())
                || !label
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        })
    {
        return Err("Expected a hostname such as flags.example.com, without scheme or port".into());
    }
    Ok(value.to_ascii_lowercase())
}

#[derive(Clone)]
pub(crate) struct Transport {
    pub(crate) tls: bool,
    pub(crate) hostname: Option<String>,
}

impl Transport {
    pub(crate) fn scheme(&self) -> &'static str {
        if self.tls { "https" } else { "http" }
    }
}
// Custom headers require a browser preflight; no cross-origin access is enabled.
fn check_request(request: &Request<Body>, transport: &Transport) -> Result<(), ApiError> {
    let invalid_host = || {
        ApiError(
            StatusCode::MISDIRECTED_REQUEST,
            "Hostname is not allowed".into(),
        )
    };
    let host_header = request
        .headers()
        .get(header::HOST)
        .map(|value| {
            value
                .to_str()
                .ok()
                .and_then(|v| v.parse::<axum::http::uri::Authority>().ok())
                .ok_or_else(invalid_host)
        })
        .transpose()?;
    // HTTP/2 carries :authority in the URI and need not include a Host header.
    let authority = request
        .uri()
        .authority()
        .or(host_header.as_ref())
        .ok_or_else(invalid_host)?;
    if host_header
        .as_ref()
        .is_some_and(|host| !host.as_str().eq_ignore_ascii_case(authority.as_str()))
        || authority.as_str().contains('@')
    {
        return Err(invalid_host());
    }
    let host = authority.host().trim_matches(['[', ']']);
    let allowed_host = host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok()
        || transport
            .hostname
            .as_ref()
            .is_some_and(|name| host.eq_ignore_ascii_case(name));
    if !allowed_host {
        return Err(invalid_host());
    }
    if request.uri().path().starts_with("/api/") {
        let headers = request.headers();
        if headers
            .get("sec-fetch-site")
            .is_some_and(|v| v == "cross-site")
        {
            return Err(ApiError(
                StatusCode::FORBIDDEN,
                "Cross-site requests are not allowed".into(),
            ));
        }
        if !matches!(
            *request.method(),
            Method::GET | Method::HEAD | Method::OPTIONS
        ) && !(request.method() == Method::POST && request.uri().path() == "/api/usage")
            && headers
                .get("x-partly-request")
                .is_none_or(|v| v != "dashboard")
        {
            return Err(ApiError(
                StatusCode::FORBIDDEN,
                "Missing X-Partly-Request: dashboard header".into(),
            ));
        }
        if let Some(origin) = headers.get(header::ORIGIN) {
            let default_port = if transport.tls { 443 } else { 80 };
            let origin_host = if authority.port_u16() == Some(default_port) {
                authority.host()
            } else {
                authority.as_str()
            };
            let expected = format!(
                "{}://{}",
                transport.scheme(),
                origin_host.to_ascii_lowercase()
            );
            if origin.as_bytes() != expected.as_bytes() {
                return Err(ApiError(
                    StatusCode::FORBIDDEN,
                    "Cross-origin requests are not allowed".into(),
                ));
            }
        }
    }
    Ok(())
}

pub(crate) async fn security(
    State(transport): State<Transport>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let mut response = match check_request(&request, &transport) {
        Ok(()) => next.run(request).await,
        Err(error) => error.into_response(),
    };
    let headers = response.headers_mut();
    if transport.tls {
        if let Some(cookie) = headers.get_mut(header::SET_COOKIE) {
            let mut value = cookie.as_bytes().to_vec();
            value.extend_from_slice(b"; Secure");
            *cookie = HeaderValue::from_bytes(&value).expect("Static cookie suffix is valid");
        }
    }
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert("content-security-policy", HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"));
    response
}

#[cfg(test)]
mod tests;
