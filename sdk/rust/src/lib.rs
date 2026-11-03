// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

//! Async policy downloads with local Rust evaluation. Reuse one client and await `close()`.

pub mod evaluator;

#[cfg(feature = "client")]
mod builder;
#[cfg(feature = "client")]
mod client;
#[cfg(feature = "client")]
mod error;
#[cfg(feature = "client")]
mod models;
#[cfg(feature = "openfeature")]
pub mod openfeature;
#[cfg(feature = "client")]
mod policy;
#[cfg(feature = "client")]
mod usage;
#[cfg(feature = "client")]
mod values;

#[cfg(feature = "client")]
pub use error::{Error, UrlParseError};
pub use evaluator::{Context, Decision, EvaluationError, Resolution};
#[cfg(feature = "client")]
pub use models::{Client, ClientBuilder};
