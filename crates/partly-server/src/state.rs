// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::{ApiError, auth, store::Store};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub(crate) struct App {
    store: Arc<Mutex<Store>>,
    pub(crate) auth: Arc<auth::Auth>,
}

impl App {
    pub(crate) fn new(store: Store) -> Self {
        Self {
            store: Arc::new(Mutex::new(store)),
            auth: Arc::new(auth::Auth::new()),
        }
    }
    pub(crate) async fn run<T, F>(&self, operation: F) -> Result<T, ApiError>
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

#[cfg(test)]
mod tests;
