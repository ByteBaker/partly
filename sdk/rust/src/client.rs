// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use crate::{
    Context, Decision, Error, Resolution,
    models::{Client, ClientBuilder, Inner},
};
use reqwest::{Certificate, redirect};
use std::sync::{Arc, atomic::Ordering};

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
        Ok(self.evaluation_policy(key)?.evaluate(key, context)?)
    }

    /// Resolve a built-in kind, or `named` with an exact expected dashboard type name.
    pub fn resolve(
        &self,
        key: &str,
        context: &Context,
        kind: &str,
        expected_type: Option<&str>,
    ) -> Result<Resolution, Error> {
        Ok(self
            .evaluation_policy(key)?
            .resolve(key, context, kind, expected_type)?)
    }

    fn evaluation_policy(&self, key: &str) -> Result<Arc<crate::evaluator::Policy>, Error> {
        let accepted = self.inner.accepted.read().unwrap();
        if self.inner.revoked.load(Ordering::Acquire) {
            return Err(Error::KeyRevoked);
        }
        let policy = Arc::clone(&accepted.as_ref().ok_or(Error::PolicyNotReady)?.policy);
        if policy.flags.contains_key(key) {
            self.inner.record_usage(key);
        }
        drop(accepted);
        Ok(policy)
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
