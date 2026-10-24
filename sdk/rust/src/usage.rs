// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use super::{Error, Inner, Policy};
use std::{
    collections::{HashSet, VecDeque},
    sync::{atomic::Ordering, Arc},
    time::{Duration, Instant},
};
use tokio::{runtime::Handle, task::JoinHandle};

const BATCH_SIZE: usize = 128;
const INTERVAL: Duration = Duration::from_secs(60);

pub(super) struct Usage {
    pending: VecDeque<String>,
    names: HashSet<String>,
    pub(super) last: Instant,
    retry: bool,
    due: bool,
    pub(super) runtime: Option<Handle>,
    pub(super) task: Option<JoinHandle<()>>,
}

impl Default for Usage {
    fn default() -> Self {
        Self {
            pending: VecDeque::new(),
            names: HashSet::new(),
            last: Instant::now(),
            retry: false,
            due: false,
            runtime: None,
            task: None,
        }
    }
}

impl Usage {
    fn add(&mut self, key: &str) {
        if !self.names.contains(key) {
            self.names.insert(key.to_owned());
            self.pending.push_back(key.to_owned());
        }
    }

    pub(super) fn prune(&mut self, policy: &Policy) {
        self.pending.retain(|key| policy.flags.contains_key(key));
        self.names.retain(|key| policy.flags.contains_key(key));
    }

    pub(super) fn clear(&mut self) {
        self.pending.clear();
        self.names.clear();
    }
}

impl Inner {
    // The caller holds the accepted-policy read lock, so refresh cannot reintroduce old names.
    pub(super) fn record_usage(self: &Arc<Self>, key: &str) {
        let mut usage = self.usage.lock().unwrap();
        if *self.closed.borrow() || self.revoked.load(Ordering::Acquire) {
            return;
        }
        usage.add(key);
        if usage.task.as_ref().is_some_and(|task| !task.is_finished())
            || (usage.last.elapsed() < INTERVAL
                && (usage.retry || (!usage.due && usage.pending.len() < BATCH_SIZE)))
        {
            return;
        }
        let Some(runtime) = usage.runtime.clone() else {
            return;
        };
        let count = usage.pending.len().min(BATCH_SIZE);
        let batch: Vec<_> = usage.pending.drain(..count).collect();
        for key in &batch {
            usage.names.remove(key);
        }
        usage.due = !usage.pending.is_empty() && (usage.due || usage.last.elapsed() >= INTERVAL);
        usage.last = Instant::now();
        let inner = Arc::clone(self);
        usage.task = Some(runtime.spawn(async move {
            inner.report_usage(batch).await;
        }));
    }

    async fn report_usage(&self, batch: Vec<String>) {
        let http = self.http.lock().unwrap().as_ref().cloned();
        let Some(http) = http else { return };
        if self.check_open().is_err() {
            return;
        }
        let mut endpoint = self.endpoint.clone();
        endpoint.set_path("/api/usage");
        let response = http
            .post(endpoint)
            .bearer_auth(&self.config.key)
            .header("x-partly-environment", &self.config.environment)
            .header("x-partly-application", &self.config.application_id)
            .header("content-type", "application/json")
            .body(serde_json::json!({"flags": &batch}).to_string())
            .send()
            .await;
        if response.as_ref().is_ok_and(|response| {
            response.status().as_u16() == 401
                && response
                    .headers()
                    .get("x-partly-error")
                    .is_some_and(|value| value == "key_revoked")
        }) {
            self.revoke();
            return;
        }
        let accepted = self.accepted.read().unwrap();
        if self.revoked.load(Ordering::Acquire) || *self.closed.borrow() {
            return;
        }
        let mut usage = self.usage.lock().unwrap();
        usage.retry = !response.is_ok_and(|response| response.status().as_u16() == 204);
        if usage.retry {
            // Failures cool down from completion, even with a full queue.
            usage.last = Instant::now();
            if let Some(accepted) = accepted.as_ref() {
                let pending = std::mem::take(&mut usage.pending);
                usage.names.clear();
                for key in batch.into_iter().chain(pending) {
                    if accepted.policy.flags.contains_key(&key) {
                        usage.add(&key);
                    }
                }
            }
        }
    }

    pub(super) fn revoke(&self) {
        let mut accepted = self.accepted.write().unwrap();
        self.revoked.store(true, Ordering::Release);
        *accepted = None;
        *self.last_error.write().unwrap() = Some(Arc::new(Error::KeyRevoked));
        let mut usage = self.usage.lock().unwrap();
        usage.clear();
        if let Some(task) = &usage.task {
            task.abort();
        }
        if let Some(task) = self.poller.lock().unwrap().as_ref() {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests;
