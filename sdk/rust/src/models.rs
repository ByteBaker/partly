// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use crate::{Error, usage};
use partly_core::Policy;
use reqwest::Url;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, RwLock, atomic::AtomicBool},
    time::Duration,
};
use tokio::{sync::watch, task::JoinHandle};

pub struct ClientBuilder {
    pub(crate) url: String,
    pub(crate) key: String,
    pub(crate) environment: String,
    pub(crate) application_id: String,
    pub(crate) interval: Duration,
    pub(crate) timeout: Duration,
    pub(crate) ca: Option<PathBuf>,
}

pub(crate) struct Accepted {
    pub(crate) policy: Arc<Policy>,
    pub(crate) etag: String,
}

pub(crate) struct Inner {
    pub(crate) config: ClientBuilder,
    pub(crate) endpoint: Url,
    pub(crate) http: Mutex<Option<reqwest::Client>>,
    pub(crate) accepted: RwLock<Option<Accepted>>,
    pub(crate) serial: tokio::sync::Mutex<()>,
    pub(crate) poller: Mutex<Option<JoinHandle<()>>>,
    pub(crate) closed: watch::Sender<bool>,
    pub(crate) revoked: AtomicBool,
    pub(crate) last_error: RwLock<Option<Arc<Error>>>,
    pub(crate) usage: Mutex<usage::Usage>,
}

pub struct Client {
    pub(crate) inner: Arc<Inner>,
}
