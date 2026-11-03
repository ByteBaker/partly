// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use crate::{
    Error,
    models::{Client, ClientBuilder, Inner},
    usage,
};
use reqwest::Url;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, RwLock, atomic::AtomicBool},
    time::Duration,
};
use tokio::sync::watch;

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
        if !crate::evaluator::valid_key(&self.environment)
            || !crate::evaluator::valid_key(&self.application_id)
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
