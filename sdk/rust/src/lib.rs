// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

//! Async policy downloads with local Rust evaluation. Reuse one client and await `close()`.

pub use partly_core::{Context, Decision, EvaluationError};
use partly_core::{Policy, MAX_POLICY_BYTES};
use reqwest::{header, redirect, Certificate, Url};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, RwLock,
    },
    time::Duration,
};
use tokio::{sync::watch, task::JoinHandle};
pub use url::ParseError as UrlParseError;

#[cfg(feature = "openfeature")]
pub mod openfeature;
mod usage;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Invalid control-plane URL: {0}")]
    UrlParse(#[from] UrlParseError),
    #[error("Invalid client configuration: {0}")]
    Configuration(&'static str),
    #[error("No policy loaded; await init first")]
    PolicyNotReady,
    #[error("API Key rejected")]
    Authentication,
    #[error("API Key revoked; build a new client with a valid key")]
    KeyRevoked,
    #[error("Invalid policy: {0}")]
    InvalidPolicy(String),
    #[error("Policy request failed: {0}")]
    Refresh(&'static str),
    #[error("Client is closed")]
    ClientClosed,
    #[error(transparent)]
    Evaluation(#[from] EvaluationError),
}

pub struct ClientBuilder {
    url: String,
    key: String,
    environment: String,
    application_id: String,
    interval: Duration,
    timeout: Duration,
    ca: Option<PathBuf>,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            url: String::new(),
            key: String::new(),
            environment: String::new(),
            application_id: String::new(),
            interval: Duration::from_secs(30),
            timeout: Duration::from_secs(5),
            ca: None,
        }
    }
}

impl ClientBuilder {
    pub fn url(mut self, value: impl Into<String>) -> Self {
        self.url = value.into();
        self
    }
    pub fn secret_key(mut self, value: impl Into<String>) -> Self {
        self.key = value.into();
        self
    }
    pub fn environment(mut self, value: impl Into<String>) -> Self {
        self.environment = value.into();
        self
    }
    pub fn application_id(mut self, value: impl Into<String>) -> Self {
        self.application_id = value.into();
        self
    }
    pub fn poll_interval(mut self, value: Duration) -> Self {
        self.interval = value;
        self
    }
    pub fn timeout(mut self, value: Duration) -> Self {
        self.timeout = value;
        self
    }
    pub fn ca_bundle(mut self, value: impl Into<PathBuf>) -> Self {
        self.ca = Some(value.into());
        self
    }
    pub fn build(self) -> Result<Client, Error> {
        let url = Url::parse(&self.url)?;
        if self.url.chars().any(|c| c <= ' ' || c == '\u{7f}')
            || self.url.contains(['\\', '@'])
            || !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err(Error::Configuration(
                "control-plane URL must be an HTTP(S) origin without credentials, a path, query, or fragment",
            ));
        }
        if self.key.is_empty() || !self.key.bytes().all(|b| (33..=126).contains(&b)) {
            return Err(Error::Configuration(
                "supply a nonempty ASCII API Key without whitespace",
            ));
        }
        if !partly_core::valid_key(&self.environment)
            || !partly_core::valid_key(&self.application_id)
        {
            return Err(Error::Configuration(
                "supply an environment name and application ID",
            ));
        }
        if self.interval.is_zero() || self.timeout.is_zero() {
            return Err(Error::Configuration(
                "interval and timeout must be positive",
            ));
        }
        if self.ca.as_ref().is_some_and(|p| p.as_os_str().is_empty()) {
            return Err(Error::Configuration("CA bundle path must not be empty"));
        }
        let (closed, _) = watch::channel(false);
        Ok(Client {
            inner: Arc::new(Inner {
                config: self,
                endpoint: url.join("api/policy")?,
                http: Mutex::new(None),
                accepted: RwLock::new(None),
                serial: tokio::sync::Mutex::new(()),
                poller: Mutex::new(None),
                closed,
                revoked: AtomicBool::new(false),
                last_error: RwLock::new(None),
                usage: Mutex::new(usage::Usage::default()),
            }),
        })
    }
}

struct Accepted {
    policy: Arc<Policy>,
    etag: String,
}

struct Inner {
    config: ClientBuilder,
    endpoint: Url,
    http: Mutex<Option<reqwest::Client>>,
    accepted: RwLock<Option<Accepted>>,
    serial: tokio::sync::Mutex<()>,
    poller: Mutex<Option<JoinHandle<()>>>,
    closed: watch::Sender<bool>,
    revoked: AtomicBool,
    last_error: RwLock<Option<Arc<Error>>>,
    usage: Mutex<usage::Usage>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(task) = self.poller.get_mut().unwrap().take() {
            task.abort();
        }
        if let Some(task) = self.usage.get_mut().unwrap().task.take() {
            task.abort();
        }
    }
}

pub struct Client {
    inner: Arc<Inner>,
}

impl Client {
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
    }

    pub async fn init(&self) -> Result<(), Error> {
        let inner = &self.inner;
        let _guard = inner.serial.lock().await;
        inner.check_open()?;
        if inner.accepted.read().unwrap().is_some() {
            return Ok(());
        }
        if inner.http.lock().unwrap().is_none() {
            let ca = inner.config.ca.clone();
            let timeout = inner.config.timeout;
            // Root loading and TLS setup may touch the filesystem.
            let http = tokio::task::spawn_blocking(move || {
                let mut builder = reqwest::Client::builder()
                    .no_proxy()
                    .redirect(redirect::Policy::none())
                    .timeout(timeout);
                if let Some(path) = ca {
                    let bytes =
                        std::fs::read(path).map_err(|_| Error::Refresh("cannot read CA bundle"))?;
                    let certs = Certificate::from_pem_bundle(&bytes)
                        .map_err(|_| Error::Refresh("invalid CA bundle"))?;
                    if certs.is_empty() {
                        return Err(Error::Refresh("empty CA bundle"));
                    }
                    builder = builder.tls_built_in_root_certs(false);
                    for cert in certs {
                        builder = builder.add_root_certificate(cert);
                    }
                }
                builder
                    .build()
                    .map_err(|_| Error::Refresh("cannot configure TLS"))
            })
            .await
            .map_err(|_| Error::Refresh("TLS setup task failed"))??;
            inner.check_open()?;
            *inner.http.lock().unwrap() = Some(http);
        }
        inner.download().await?;
        inner.check_open()?;
        {
            let mut usage = inner.usage.lock().unwrap();
            usage.runtime = Some(tokio::runtime::Handle::current());
            usage.last = std::time::Instant::now();
        }
        let weak = Arc::downgrade(inner);
        let interval = inner.config.interval;
        let task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(interval).await;
                let Some(inner) = weak.upgrade() else { break };
                let _guard = inner.serial.lock().await;
                if inner.check_open().is_err() {
                    break;
                }
                if let Err(error) = inner.download().await {
                    let revoked = matches!(error, Error::KeyRevoked);
                    {
                        let _accepted = inner.accepted.read().unwrap();
                        if !inner.revoked.load(Ordering::Acquire) {
                            *inner.last_error.write().unwrap() = Some(Arc::new(error));
                        }
                    }
                    if revoked {
                        break;
                    }
                }
            }
        });
        *inner.poller.lock().unwrap() = Some(task);
        Ok(())
    }

    pub async fn refresh(&self) -> Result<bool, Error> {
        let _guard = self.inner.serial.lock().await;
        self.inner.check_open()?;
        if self.inner.accepted.read().unwrap().is_none() {
            return Err(Error::PolicyNotReady);
        }
        self.inner.download().await
    }

    pub fn evaluate(&self, key: &str, context: &Context) -> Result<Decision, Error> {
        let accepted = self.inner.accepted.read().unwrap();
        if self.inner.revoked.load(Ordering::Acquire) {
            return Err(Error::KeyRevoked);
        }
        let policy = Arc::clone(&accepted.as_ref().ok_or(Error::PolicyNotReady)?.policy);
        if policy.flags.contains_key(key) {
            self.inner.record_usage(key);
        }
        drop(accepted);
        Ok(policy.evaluate(key, context)?)
    }

    pub fn last_refresh_error(&self) -> Option<Arc<Error>> {
        self.inner.last_error.read().unwrap().clone()
    }

    /// Query the accepted policy without evaluating context. Unavailable policies return an error.
    pub fn has_flag(&self, key: &str) -> Result<bool, Error> {
        let accepted = self.inner.accepted.read().unwrap();
        if self.inner.revoked.load(Ordering::Acquire) {
            return Err(Error::KeyRevoked);
        }
        let policy = &accepted.as_ref().ok_or(Error::PolicyNotReady)?.policy;
        Ok(policy.flags.contains_key(key))
    }

    pub async fn close(&self) -> Result<(), Error> {
        self.inner.closed.send_replace(true);
        // Wait for init to finish registering its poller before taking the handle.
        let _guard = self.inner.serial.lock().await;
        let task = self.inner.poller.lock().unwrap().take();
        if let Some(task) = task {
            task.abort();
            let _ = task.await;
        }
        let usage_task = {
            let mut usage = self.inner.usage.lock().unwrap();
            usage.clear();
            usage.task.take()
        };
        if let Some(task) = usage_task {
            task.abort();
            let _ = task.await;
        }
        self.inner.http.lock().unwrap().take();
        Ok(())
    }
}

impl Inner {
    fn check_open(&self) -> Result<(), Error> {
        if self.revoked.load(Ordering::Acquire) {
            return Err(Error::KeyRevoked);
        }
        if *self.closed.borrow() {
            Err(Error::ClientClosed)
        } else {
            Ok(())
        }
    }

    async fn download(&self) -> Result<bool, Error> {
        let mut closed = self.closed.subscribe();
        self.check_open()?;
        let result = tokio::select! {
            biased;
            _ = closed.changed() => Err(Error::ClientClosed),
            result = self.request() => result,
        };
        let _accepted = self.accepted.read().unwrap();
        if self.revoked.load(Ordering::Acquire) {
            Err(Error::KeyRevoked)
        } else {
            result
        }
    }

    async fn request(&self) -> Result<bool, Error> {
        let http = self.http.lock().unwrap().as_ref().unwrap().clone();
        let etag = self
            .accepted
            .read()
            .unwrap()
            .as_ref()
            .map(|a| a.etag.clone());
        let mut request = http
            .get(self.endpoint.clone())
            .bearer_auth(&self.config.key)
            .header("x-partly-environment", &self.config.environment)
            .header("x-partly-application", &self.config.application_id);
        if let Some(etag) = &etag {
            request = request.header(header::IF_NONE_MATCH, etag);
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| Error::Refresh("network or TLS failure"))?;
        let status = response.status().as_u16();
        if status == 401
            && response
                .headers()
                .get("x-partly-error")
                .is_some_and(|v| v == "key_revoked")
        {
            self.revoke();
            return Err(Error::KeyRevoked);
        }
        if matches!(status, 401 | 403) {
            return Err(Error::Authentication);
        }
        let tag = response
            .headers()
            .get(header::ETAG)
            .map(|value| value.to_str().map(str::to_owned))
            .transpose()
            .map_err(|_| Error::InvalidPolicy("invalid ETag".into()))?;
        if status == 304 {
            if etag.is_none() || tag.as_ref().is_some_and(|t| Some(t) != etag.as_ref()) {
                return Err(Error::InvalidPolicy(
                    "unchanged response has no matching policy".into(),
                ));
            }
            let _accepted = self.accepted.read().unwrap();
            self.check_open()?;
            *self.last_error.write().unwrap() = None;
            return Ok(false);
        }
        if status != 200 {
            return Err(Error::Refresh("unexpected HTTP status"));
        }
        let tag = tag
            .filter(|t| valid_etag(t))
            .ok_or_else(|| Error::InvalidPolicy("missing or invalid ETag".into()))?;
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| Error::Refresh("incomplete download"))?
        {
            if body.len() + chunk.len() > MAX_POLICY_BYTES {
                return Err(Error::InvalidPolicy("policy exceeds 1 MiB".into()));
            }
            body.extend_from_slice(&chunk);
        }
        let policy = tokio::task::spawn_blocking(move || Policy::from_json(&body))
            .await
            .map_err(|_| Error::Refresh("validation task failed"))?
            .map_err(|_| Error::InvalidPolicy("policy validation failed".into()))?;
        self.check_open()?;
        if policy.environment != self.config.environment
            || policy.application_id != self.config.application_id
        {
            return Err(Error::InvalidPolicy(
                "policy does not match configured scope".into(),
            ));
        }
        let mut accepted = self.accepted.write().unwrap();
        self.check_open()?;
        if accepted.as_ref().is_some_and(|a| {
            a.policy.project != policy.project || a.policy.environment != policy.environment
        }) {
            return Err(Error::InvalidPolicy("policy scope changed".into()));
        }
        self.usage.lock().unwrap().prune(&policy);
        *accepted = Some(Accepted {
            policy: Arc::new(policy),
            etag: tag,
        });
        *self.last_error.write().unwrap() = None;
        Ok(true)
    }
}

fn valid_etag(tag: &str) -> bool {
    tag.len() >= 3
        && tag.starts_with('"')
        && tag.ends_with('"')
        && tag.as_bytes()[1..tag.len() - 1]
            .iter()
            .all(|b| (33..=126).contains(b) && *b != b'"')
}
