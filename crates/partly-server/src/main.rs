mod store;

use axum::{
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Path, State},
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
    net::IpAddr,
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
    Serve {
        #[arg(long, default_value = "127.0.0.1")]
        host: IpAddr,
        #[arg(long, default_value_t = 8080)]
        port: u16,
        #[arg(long, default_value = "./partly.db")]
        db: PathBuf,
    },
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
}

impl From<rusqlite::Error> for ApiError {
    fn from(error: rusqlite::Error) -> Self {
        Self::internal(format!("Database operation failed: {error}"))
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let message = if self.0.is_server_error() {
            eprintln!("{}", self.1);
            "Storage operation failed. Check the server terminal.".to_owned()
        } else {
            self.1
        };
        (self.0, Json(json!({"error": message}))).into_response()
    }
}

#[derive(Clone)]
struct App(Arc<Mutex<Store>>);

impl App {
    async fn run<T, F>(&self, operation: F) -> Result<T, ApiError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Store) -> Result<T, ApiError> + Send + 'static,
    {
        let store = self.0.clone();
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
fn check_request(request: &Request<Body>) -> Result<(), ApiError> {
    // The local POC accepts direct IPs and localhost, preventing browser DNS rebinding.
    let allowed_host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<axum::http::uri::Authority>().ok())
        .is_some_and(|authority| {
            let host = authority.host().trim_matches(['[', ']']);
            host.eq_ignore_ascii_case("localhost") || host.parse::<IpAddr>().is_ok()
        });
    if !allowed_host {
        return Err(ApiError(
            StatusCode::MISDIRECTED_REQUEST,
            "Use localhost or a direct IP address".into(),
        ));
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
            let expected = format!(
                "http://{}",
                headers
                    .get(header::HOST)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
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

async fn security(request: Request<Body>, next: Next) -> Response {
    let mut response = match check_request(&request) {
        Ok(()) => next.run(request).await,
        Err(error) => error.into_response(),
    };
    let headers = response.headers_mut();
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
    app.run(move |s| {
        s.setup(&c.username, &c.password)?;
        Ok(login_response(&s.new_session()))
    })
    .await
}

async fn login(State(app): State<App>, Json(c): Json<Credentials>) -> Result<Response, ApiError> {
    app.run(move |s| Ok(login_response(&s.login(&c.username, &c.password)?)))
        .await
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
        s.delete_flag(&env, &key)?;
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}

async fn keys(
    State(app): State<App>,
    headers: HeaderMap,
    Path(env): Path<String>,
) -> Result<Json<Value>, ApiError> {
    app.run(move |s| {
        s.authorize(&session(&headers))?;
        Ok(Json(json!({"keys": s.keys(&env)?})))
    })
    .await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyName {
    name: String,
}

async fn create_key(
    State(app): State<App>,
    headers: HeaderMap,
    Path(env): Path<String>,
    Json(input): Json<KeyName>,
) -> Result<Json<Value>, ApiError> {
    app.run(move |s| {
        s.authorize(&session(&headers))?;
        Ok(Json(json!(s.create_key(&env, &input.name)?)))
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
        s.rename_key(&id, &input.name)?;
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
        s.revoke_key(&id)?;
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
        let env = s.key_environment(&key)?;
        let body = serde_json::to_vec(&s.policy(&env)?)
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
    if !matches!(screen.as_str(), "flags" | "keys" | "docs") {
        return Err(ApiError::not_found("Unknown screen"));
    }
    app.run(move |s| s.environment(&env)).await?;
    Ok(dashboard().await.into_response())
}

#[tokio::main]
async fn main() {
    if let Err(error) = serve().await {
        eprintln!("Partly could not start: {error}");
        std::process::exit(1);
    }
}

async fn serve() -> Result<(), Box<dyn std::error::Error>> {
    let Cli {
        command: Command::Serve { host, port, db },
    } = Cli::parse();
    let store = Store::open(db).map_err(|e| e.1)?;
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
        .route("/api/environments/{env}/keys", get(keys).post(create_key))
        .route("/api/keys/{id}", put(rename_key).delete(revoke_key))
        .route("/api/policy", get(policy))
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(middleware::from_fn(security))
        .with_state(App(Arc::new(Mutex::new(store))));
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    println!("Partly listening at http://{}", listener.local_addr()?);
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
