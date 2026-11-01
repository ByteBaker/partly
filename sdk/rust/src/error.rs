// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use crate::EvaluationError;
pub use url::ParseError as UrlParseError;

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
