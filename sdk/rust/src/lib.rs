// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

//! Async policy downloads with local Rust evaluation. Reuse one client and await `close()`.

mod builder;
mod client;
mod error;
mod models;
#[cfg(feature = "openfeature")]
pub mod openfeature;
mod policy;
mod usage;
mod values;

pub use error::{Error, UrlParseError};
pub use models::{Client, ClientBuilder};
pub use partly_core::{Context, Decision, EvaluationError, Resolution};
