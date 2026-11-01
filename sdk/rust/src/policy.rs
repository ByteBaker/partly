// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use crate::{
    Error,
    models::{Accepted, Inner},
};
use partly_core::{MAX_POLICY_BYTES, Policy};
use reqwest::header;
use std::sync::{Arc, atomic::Ordering};

impl Inner {
    pub(crate) fn check_open(&self) -> Result<(), Error> {
        if self.revoked.load(Ordering::Acquire) {
            return Err(Error::KeyRevoked);
        }
        if *self.closed.borrow() {
            Err(Error::ClientClosed)
        } else {
            Ok(())
        }
    }

    pub(crate) async fn download(&self) -> Result<bool, Error> {
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
