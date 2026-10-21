// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

mod auth;
mod store;

use axum::{
    body::{Body, Bytes},
    extract::{ConnectInfo, DefaultBodyLimit, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, Method, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post, put},
    Json, Router,
};
use clap::{Parser, Subcommand};
use partly_core::Flag;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    io::{IsTerminal, Read},
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::{Arc, Mutex},
};
use store::Store;

#[derive(Parser)]
#[command(name = "partly", version, about = "Local feature flag control plane")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Reset the local administrator password from one line on standard input. Stop the server first.
    ResetPassword {
        #[arg(long, default_value = "./partly.db")]
        db: PathBuf,
    },
    Serve {
        #[arg(long, default_value = "127.0.0.1")]
        host: IpAddr,
        #[arg(long, default_value_t = 8080)]
        port: u16,
        #[arg(long, default_value = "./partly.db")]
        db: PathBuf,
        /// PEM certificate chain for native HTTPS. Requires --tls-key.
        #[arg(long, requires = "tls_key")]
        tls_cert: Option<PathBuf>,
        /// PEM private key matching --tls-cert.
        #[arg(long, requires = "tls_cert")]
        tls_key: Option<PathBuf>,
        /// Additional DNS hostname to accept, without scheme or port.
        #[arg(long, value_parser = parse_hostname)]
        hostname: Option<String>,
    },
}

fn parse_hostname(value: &str) -> Result<String, String> {
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
struct Transport {
    tls: bool,
    hostname: Option<String>,
}

impl Transport {
    fn scheme(&self) -> &'static str {
        if self.tls {
            "https"
        } else {
            "http"
        }
    }
}

#[derive(Debug)]
pub struct ApiError(StatusCode, String);

impl ApiError {
    fn bad(message: impl Into<String>) -> Self {
        Self(StatusCode::BAD_REQUEST, message.into())
    }
    fn conflict(message: impl Into<String>) -> Self {
        Self(StatusCode::CONFLICT, message.into())
    }
    fn not_found(message: impl Into<String>) -> Self {
        Self(StatusCode::NOT_FOUND, message.into())
    }
    fn internal(message: impl Into<String>) -> Self {
        Self(StatusCode::INTERNAL_SERVER_ERROR, message.into())
    }
    fn unauthorized() -> Self {
        Self(
            StatusCode::UNAUTHORIZED,
            "Authentication required or credentials invalid".into(),
        )
    }
    fn throttled() -> Self {
        Self(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many sign-in attempts or password workers busy. Retry after 60 seconds.".into(),
        )
    }
}

impl From<rusqlite::Error> for ApiError {
    fn from(error: rusqlite::Error) -> Self {
        Self::internal(format!("Database operation failed: {error}"))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let revoked = self.0 == StatusCode::UNAUTHORIZED && self.1 == "key_revoked";
        let message = if self.0.is_server_error() {
            eprintln!("{}", self.1);
            "Storage operation failed. Check the server terminal.".to_owned()
        } else {
            self.1
        };
        let mut response = (self.0, Json(json!({"error": message}))).into_response();
        if revoked {
            response = (
                self.0,
                Json(json!({"error": "API Key revoked", "code": "key_revoked"})),
            )
                .into_response();
            response
                .headers_mut()
                .insert("x-partly-error", HeaderValue::from_static("key_revoked"));
        }
        if self.0 == StatusCode::TOO_MANY_REQUESTS {
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("60"));
        }
        response
    }
}

#[derive(Clone)]
struct App {
    store: Arc<Mutex<Store>>,
    auth: Arc<auth::Auth>,
}

impl App {
    fn new(store: Store) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
            auth: Arc::new(auth::Auth::new()),
        }
    }
    async fn run<T, F>(&self, operation: F) -> Result<T, ApiError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Store) -> Result<T, ApiError> + Send + 'static,
    {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || {
            let mut store = store
                .lock()
                .map_err(|_| ApiError::internal("Storage lock failed"))?;
            operation(&mut store)
        })
        .await
        .map_err(|_| ApiError::internal("Storage task failed"))?
    }
}

fn session(headers: &HeaderMap) -> String {
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
        Json(json!({"ok": true})),
    )
        .into_response()
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
        ) && headers
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

async fn security(
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credentials {
    username: String,
    password: String,
}

async fn status(State(app): State<App>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    app.run(move |s| Ok(Json(json!({"setup_required": !s.ready(), "authenticated": s.authenticated(&session(&headers))})))).await
}

async fn setup(State(app): State<App>, Json(c): Json<Credentials>) -> Result<Response, ApiError> {
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
        Ok(login_response(&s.new_session()))
    })
    .await
}

async fn login(
    State(app): State<App>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Json(c): Json<Credentials>,
) -> Result<Response, ApiError> {
    app.auth.check_attempt(peer.ip())?;
    if c.password.len() > 1024 || c.username.len() > 64 {
        return Err(ApiError::unauthorized());
    }
    let (username, hash) = app.run(|s| s.credentials()).await?;
    let valid = app
        .auth
        .run(move || {
            let valid = auth::verify_password(&c.password, &hash)?;
            Ok(valid && username == c.username)
        })
        .await?;
    if !valid {
        return Err(ApiError::unauthorized());
    }
    app.run(|s| Ok(login_response(&s.new_session()))).await
}

async fn logout(State(app): State<App>, headers: HeaderMap) -> Result<Response, ApiError> {
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

async fn environments(State(app): State<App>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    app.run(move |s| {
        s.authorize(&session(&headers))?;
        Ok(Json(
            json!({"project": "default", "environments": ["development", "production"]}),
        ))
    })
    .await
}

async fn flags(
    State(app): State<App>,
    headers: HeaderMap,
    Path(env): Path<String>,
) -> Result<Json<Value>, ApiError> {
    app.run(move |s| {
        s.authorize(&session(&headers))?;
        Ok(Json(json!(s.policy(&env)?.flags)))
    })
    .await
}

#[derive(Deserialize, Serialize)]
struct SavedFlag {
    key: String,
    #[serde(flatten)]
    flag: Flag,
}

fn parse_flag<T: DeserializeOwned>(headers: &HeaderMap, body: &[u8]) -> Result<T, ApiError> {
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if content_type != "application/json"
        && !(content_type.starts_with("application/") && content_type.ends_with("+json"))
    {
        return Err(ApiError(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "Expected an application/json body".into(),
        ));
    }
    let input = Json::<T>::from_bytes(body)
        .map(|Json(input)| input)
        .map_err(|error| ApiError(error.status(), error.body_text()))?;
    partly_core::validate_json_number_limits(body)
        .map_err(|error| ApiError(StatusCode::UNPROCESSABLE_ENTITY, error))?;
    Ok(input)
}

async fn create_flag(
    State(app): State<App>,
    headers: HeaderMap,
    Path(env): Path<String>,
    body: Bytes,
) -> Result<(StatusCode, Json<SavedFlag>), ApiError> {
    app.run(move |s| {
        s.authorize(&session(&headers))?;
        // Rule validation compiles regex; do it after auth on the blocking worker.
        let input: SavedFlag = parse_flag(&headers, &body)?;
        s.save_flag(&env, &input.key, &input.flag, true)?;
        Ok((StatusCode::CREATED, Json(input)))
    })
    .await
}

async fn save_flag(
    State(app): State<App>,
    headers: HeaderMap,
    Path((env, key)): Path<(String, String)>,
    body: Bytes,
) -> Result<Json<SavedFlag>, ApiError> {
    app.run(move |s| {
        s.authorize(&session(&headers))?;
        let flag: Flag = parse_flag(&headers, &body)?;
        if !partly_core::valid_key(&key) {
            return Err(ApiError::bad("Invalid flag key"));
        }
        flag.validate().map_err(ApiError::bad)?;
        check_flag_confirmation(s, &headers, &env, &key)?;
        s.save_flag(&env, &key, &flag, false)?;
        Ok(Json(SavedFlag { key, flag }))
    })
    .await
}

async fn delete_flag(
    State(app): State<App>,
    headers: HeaderMap,
    Path((env, key)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    app.run(move |s| {
        s.authorize(&session(&headers))?;
        check_flag_confirmation(s, &headers, &env, &key)?;
        s.delete_flag(&env, &key)?;
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}

fn flag_impact_value(s: &Store, env: &str, key: &str) -> Result<Value, ApiError> {
    let apps = s.flag_impact(env, key)?;
    let policy = s.policy(env)?;
    let bytes = serde_json::to_vec(&json!({"flag": policy.flags.get(key), "applications": apps}))
        .map_err(|_| ApiError::internal("Cannot encode impact"))?;
    Ok(json!({"applications": apps, "token": format!("{:x}", Sha256::digest(bytes))}))
}

fn check_flag_confirmation(
    s: &Store,
    headers: &HeaderMap,
    env: &str,
    key: &str,
) -> Result<(), ApiError> {
    let impact = flag_impact_value(s, env, key)?;
    if headers
        .get("x-partly-confirm")
        .and_then(|v| v.to_str().ok())
        != Some(format!("{env}/{key}").as_str())
    {
        return Err(ApiError::bad("Confirm the exact environment/flag name"));
    }
    if headers.get("x-partly-impact").and_then(|v| v.to_str().ok()) != impact["token"].as_str() {
        return Err(ApiError::conflict("The flag or its applications changed. Review the affected applications and confirm again."));
    }
    Ok(())
}

async fn flag_impact(
    State(app): State<App>,
    headers: HeaderMap,
    Path((env, key)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    app.run(move |s| {
        s.authorize(&session(&headers))?;
        Ok(Json(flag_impact_value(s, &env, &key)?))
    })
    .await
}

async fn applications(
    State(app): State<App>,
    headers: HeaderMap,
    Path(env): Path<String>,
) -> Result<Json<Value>, ApiError> {
    app.run(move |s| {
        s.authorize(&session(&headers))?;
        Ok(Json(json!({"applications": s.applications(&env)?})))
    })
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplicationInput {
    name: String,
    flags: Vec<String>,
}

async fn create_application(
    State(app): State<App>,
    headers: HeaderMap,
    Path(env): Path<String>,
    Json(input): Json<ApplicationInput>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    app.run(move |s| {
        s.authorize(&session(&headers))?;
        Ok((
            StatusCode::CREATED,
            Json(json!(s.save_application(
                &env,
                None,
                &input.name,
                &input.flags
            )?)),
        ))
    })
    .await
}

async fn save_application(
    State(app): State<App>,
    headers: HeaderMap,
    Path((env, id)): Path<(String, String)>,
    Json(input): Json<ApplicationInput>,
) -> Result<Json<Value>, ApiError> {
    app.run(move |s| {
        s.authorize(&session(&headers))?;
        Ok(Json(json!(s.save_application(
            &env,
            Some(&id),
            &input.name,
            &input.flags
        )?)))
    })
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct AuditQuery {
    before: Option<i64>,
    action: Option<String>,
    application_id: Option<String>,
}

async fn audits(
    State(app): State<App>,
    headers: HeaderMap,
    Path(env): Path<String>,
    Query(query): Query<AuditQuery>,
) -> Result<Json<Value>, ApiError> {
    app.run(move |s| { s.authorize(&session(&headers))?; Ok(Json(json!({"audits": s.audits(&env, query.before, query.action.as_deref(), query.application_id.as_deref())?}))) }).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyName {
    name: String,
}

async fn create_key(
    State(app): State<App>,
    headers: HeaderMap,
    Path((env, id)): Path<(String, String)>,
    Json(input): Json<KeyName>,
) -> Result<Json<Value>, ApiError> {
    app.run(move |s| {
        s.authorize(&session(&headers))?;
        Ok(Json(json!(s.create_key(&env, &id, &input.name)?)))
    })
    .await
}

async fn rename_key(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<KeyName>,
) -> Result<StatusCode, ApiError> {
    app.run(move |s| {
        s.authorize(&session(&headers))?;
        s.change_key(&id, Some(&input.name))?;
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}

async fn revoke_key(
    State(app): State<App>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiError> {
    app.run(move |s| {
        s.authorize(&session(&headers))?;
        s.change_key(&id, None)?;
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}

async fn policy(State(app): State<App>, headers: HeaderMap) -> Result<Response, ApiError> {
    let key = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or_else(ApiError::unauthorized)?
        .to_owned();
    app.run(move |s| {
        let env = headers
            .get("x-partly-environment")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let application = headers
            .get("x-partly-application")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let body = serde_json::to_vec(&s.app_policy(&key, env, application)?)
            .map_err(|_| ApiError::internal("Cannot encode policy"))?;
        let etag = format!("\"{:x}\"", Sha256::digest(&body));
        let matches = headers
            .get(header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| {
                v.split(',')
                    .any(|v| v.trim().trim_start_matches("W/") == etag || v.trim() == "*")
            });
        let mut response = if matches {
            StatusCode::NOT_MODIFIED.into_response()
        } else {
            ([(header::CONTENT_TYPE, "application/json")], body).into_response()
        };
        response
            .headers_mut()
            .insert(header::ETAG, HeaderValue::from_str(&etag).unwrap());
        Ok(response)
    })
    .await
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
    if !matches!(screen.as_str(), "flags" | "apps" | "audits" | "docs") {
        return Err(ApiError::not_found("Unknown screen"));
    }
    app.run(move |s| s.environment(&env)).await?;
    Ok(dashboard().await.into_response())
}

#[tokio::main]
async fn main() {
    let result = match Cli::parse().command {
        Command::ResetPassword { db } => reset_password(db),
        Command::Serve {
            host,
            port,
            db,
            tls_cert,
            tls_key,
            hostname,
        } => serve(host, port, db, tls_cert, tls_key, hostname).await,
    };
    if let Err(error) = result {
        eprintln!("Partly could not start: {error}");
        std::process::exit(1);
    }
}

fn reset_password(db: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let store = Store::open(db).map_err(|e| e.1)?;
    if !store.ready() {
        return Err("Database does not exist; complete setup first".into());
    }
    if std::io::stdin().is_terminal() {
        return Err("Read the password privately and pipe one line to reset-password; terminal input would expose it".into());
    }
    let mut password = String::new();
    std::io::stdin().take(1027).read_to_string(&mut password)?;
    if password.ends_with('\n') {
        password.pop();
        if password.ends_with('\r') {
            password.pop();
        }
    }
    if password.contains(['\r', '\n']) {
        return Err("Supply exactly one password line on standard input".into());
    }
    let hash = auth::hash_password(&password).map_err(|e| e.1)?;
    store.reset_password(&hash).map_err(|e| e.1)?;
    println!("Administrator password reset. Start the server and sign in with the new password.");
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("Could not register SIGTERM handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

async fn serve(
    host: IpAddr,
    port: u16,
    db: PathBuf,
    tls_cert: Option<PathBuf>,
    tls_key: Option<PathBuf>,
    hostname: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let tls_config = if let (Some(cert), Some(key)) = (tls_cert, tls_key) {
        rustls::crypto::ring::default_provider()
            .install_default()
            .map_err(|_| "Could not initialize the TLS crypto provider")?;
        Some(axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key).await?)
    } else {
        None
    };
    let transport = Transport {
        tls: tls_config.is_some(),
        hostname,
    };
    let scheme = transport.scheme();
    let store = Store::open(db).map_err(|e| e.1)?;
    if !store.ready() && !host.is_loopback() {
        return Err("Complete setup locally first: run with --host 127.0.0.1, create the administrator, then restart with the remote bind address".into());
    }
    let app = Router::new()
        .route("/", get(dashboard))
        .route("/environments/{env}/{screen}", get(environment_dashboard))
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
            "/docs/python.py",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
                    include_str!("../../../examples/python/quickstart.py"),
                )
            }),
        )
        .route("/api/status", get(status))
        .route("/api/setup", post(setup))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/environments", get(environments))
        .route(
            "/api/environments/{env}/flags",
            get(flags).post(create_flag),
        )
        .route(
            "/api/environments/{env}/flags/{key}",
            put(save_flag).delete(delete_flag),
        )
        .route(
            "/api/environments/{env}/flags/{key}/impact",
            get(flag_impact),
        )
        .route(
            "/api/environments/{env}/apps",
            get(applications).post(create_application),
        )
        .route("/api/environments/{env}/apps/{id}", put(save_application))
        .route("/api/environments/{env}/apps/{id}/keys", post(create_key))
        .route("/api/environments/{env}/audits", get(audits))
        .route("/api/keys/{id}", put(rename_key).delete(revoke_key))
        .route("/api/policy", get(policy))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(middleware::from_fn_with_state(transport, security))
        .with_state(App::new(store));
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    println!("Partly listening at {scheme}://{}", listener.local_addr()?);
    let handle = axum_server::Handle::new();
    let shutdown = handle.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        shutdown.graceful_shutdown(Some(std::time::Duration::from_secs(3)));
    });
    let listener = listener.into_std()?;
    let service = app.into_make_service_with_connect_info::<SocketAddr>();
    if let Some(tls) = tls_config {
        axum_server::from_tcp_rustls(listener, tls)?
            .handle(handle)
            .serve(service)
            .await?;
    } else {
        axum_server::from_tcp(listener)?
            .handle(handle)
            .serve(service)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn blocked_password_workers_do_not_hold_storage_or_queue_more_hashes() {
        let path = std::env::temp_dir().join(format!("partly-test-{}", store::secret()));
        let app = App::new(Store::open(path).unwrap());
        let mut releases = Vec::new();
        let mut tasks = Vec::new();
        for _ in 0..2 {
            let (started, ready) = tokio::sync::oneshot::channel();
            let (release, wait) = std::sync::mpsc::channel();
            let auth = app.auth.clone();
            tasks.push(tokio::spawn(async move {
                auth.run(move || {
                    started.send(()).unwrap();
                    wait.recv().unwrap();
                    Ok(())
                })
                .await
            }));
            releases.push(release);
            ready.await.unwrap();
        }
        assert_eq!(
            app.auth.run(|| Ok(())).await.unwrap_err().0,
            StatusCode::TOO_MANY_REQUESTS
        );
        let read = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            app.run(|s| Ok(s.ready())),
        )
        .await;
        // Release workers before asserting so a regression cannot hang test shutdown.
        for release in releases {
            release.send(()).unwrap();
        }
        for task in tasks {
            task.await.unwrap().unwrap();
        }
        assert!(!read.expect("Password work held the store lock").unwrap());
        app.auth.run(|| Ok(())).await.unwrap();
    }

    #[test]
    fn https_accepts_http2_authority_and_rejects_conflicting_host() {
        let transport = Transport {
            tls: true,
            hostname: Some("flags.example.com".into()),
        };
        let mut request = Request::builder()
            .uri("https://flags.example.com:8443/api/status")
            .version(axum::http::Version::HTTP_2)
            .header(header::ORIGIN, "https://flags.example.com:8443")
            .body(Body::empty())
            .unwrap();
        assert!(check_request(&request, &transport).is_ok());
        request
            .headers_mut()
            .insert(header::HOST, HeaderValue::from_static("localhost:8443"));
        assert_eq!(
            check_request(&request, &transport).unwrap_err().0,
            StatusCode::MISDIRECTED_REQUEST
        );
        request.headers_mut().remove(header::HOST);
        request.headers_mut().insert(
            header::ORIGIN,
            HeaderValue::from_static("http://flags.example.com:8443"),
        );
        request
            .headers_mut()
            .insert("x-forwarded-proto", HeaderValue::from_static("http"));
        assert_eq!(
            check_request(&request, &transport).unwrap_err().0,
            StatusCode::FORBIDDEN
        );
    }

    #[test]
    fn hostname_configuration_is_explicit_and_has_no_scheme_or_port() {
        assert_eq!(
            parse_hostname("Flags.Example.com").unwrap(),
            "flags.example.com"
        );
        for invalid in [
            "",
            "https://flags.example.com",
            "flags.example.com:443",
            "user@host",
            "*.example.com",
            "a..b",
            "-a.com",
            "a-.com",
            "a/b",
            "a b",
            "a\\nb",
        ] {
            assert!(parse_hostname(invalid).is_err(), "{invalid}");
        }
        let request = Request::builder()
            .uri("/api/status")
            .header(header::HOST, "flags.example.com:8443")
            .body(Body::empty())
            .unwrap();
        let transport = Transport {
            tls: true,
            hostname: None,
        };
        assert_eq!(
            check_request(&request, &transport).unwrap_err().0,
            StatusCode::MISDIRECTED_REQUEST
        );
    }

    #[test]
    fn https_origin_omits_default_port_and_lowercases_hostname() {
        let transport = Transport {
            tls: true,
            hostname: Some("flags.example.com".into()),
        };
        let request = Request::builder()
            .uri("/api/status")
            .header(header::HOST, "Flags.Example.com:443")
            .header(header::ORIGIN, "https://flags.example.com")
            .body(Body::empty())
            .unwrap();
        assert!(check_request(&request, &transport).is_ok());
    }
}
