// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use std::path::{Path, PathBuf};

mod api;
#[cfg(feature = "browser")]
pub mod browser;
pub mod cli;
pub mod driver;
pub mod http;
mod server;
mod tls;

pub use api::{Api, app_key, policy_headers, status, text};
pub use server::{Server, fixture};
pub use tls::{Tls, connection};

pub type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub const PASSWORD: &str = "local-test-password";

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

pub fn binary() -> PathBuf {
    std::env::var_os("PARTLY_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("target/debug/partly"))
}
