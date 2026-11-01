// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

//! Borrow an initialized client. The application owns its async initialization and cleanup.

use crate::{Client, Context, Decision, Error};
use async_trait::async_trait;
use open_feature::{
    EvaluationContext, EvaluationContextFieldValue as Field, EvaluationError,
    EvaluationErrorCode as Code, EvaluationReason, EvaluationResult, StructValue,
    provider::{FeatureProvider, ProviderMetadata, ProviderStatus, ResolutionDetails},
};
use std::sync::Arc;

pub struct PartlyProvider {
    client: Arc<Client>,
    metadata: ProviderMetadata,
    ready: Option<bool>,
}

impl PartlyProvider {
    pub fn new(client: Arc<Client>) -> Self {
        Self {
            client,
            metadata: ProviderMetadata::new("Partly"),
            ready: None,
        }
    }

    fn check(&self, key: &str) -> EvaluationResult<()> {
        if self.ready != Some(true) {
            return Err(error(Code::ProviderNotReady));
        }
        if !self.client.has_flag(key).map_err(map_error)? {
            return Err(error(Code::FlagNotFound));
        }
        Ok(())
    }

    fn unsupported<T>(&self, key: &str) -> EvaluationResult<ResolutionDetails<T>> {
        self.check(key)?;
        Err(error(Code::TypeMismatch))
    }
}

fn error(code: Code) -> EvaluationError {
    EvaluationError {
        code,
        message: None,
    }
}

fn map_error(value: Error) -> EvaluationError {
    error(match value {
        Error::PolicyNotReady => Code::ProviderNotReady,
        // The upstream Rust SDK has no dedicated ProviderFatal variant.
        Error::KeyRevoked => Code::General("PROVIDER_FATAL".into()),
        Error::Evaluation(crate::EvaluationError::FlagNotFound(_)) => Code::FlagNotFound,
        Error::Evaluation(_) => Code::InvalidContext,
        _ => Code::General("Partly evaluation failed".into()),
    })
}

#[async_trait]
impl FeatureProvider for PartlyProvider {
    async fn initialize(&mut self, _: &EvaluationContext) {
        self.ready = Some(self.client.has_flag("").is_ok());
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    fn status(&self) -> ProviderStatus {
        match self.ready {
            Some(true) => ProviderStatus::Ready,
            Some(false) => ProviderStatus::Error,
            None => ProviderStatus::NotReady,
        }
    }

    async fn resolve_bool_value(
        &self,
        key: &str,
        context: &EvaluationContext,
    ) -> EvaluationResult<ResolutionDetails<bool>> {
        self.check(key)?;
        let mut attributes = Context::new();
        for (key, field) in &context.custom_fields {
            let value = match field {
                Field::Bool(value) => serde_json::Value::Bool(*value),
                Field::Int(value) => (*value).into(),
                Field::Float(value) => serde_json::Number::from_f64(*value)
                    .ok_or_else(|| error(Code::InvalidContext))?
                    .into(),
                Field::String(value) => value.clone().into(),
                Field::DateTime(_) | Field::Struct(_) => continue,
            };
            attributes.insert(key.clone(), value);
        }
        if let Some(key) = &context.targeting_key {
            attributes.insert("id".into(), key.clone().into());
        }
        let value = self.client.evaluate(key, &attributes).map_err(map_error)? == Decision::Enabled;
        Ok(ResolutionDetails {
            value,
            reason: Some(EvaluationReason::Cached),
            variant: Some(if value { "enabled" } else { "disabled" }.into()),
            flag_metadata: None,
        })
    }

    async fn resolve_int_value(
        &self,
        key: &str,
        _: &EvaluationContext,
    ) -> EvaluationResult<ResolutionDetails<i64>> {
        self.unsupported(key)
    }
    async fn resolve_float_value(
        &self,
        key: &str,
        _: &EvaluationContext,
    ) -> EvaluationResult<ResolutionDetails<f64>> {
        self.unsupported(key)
    }
    async fn resolve_string_value(
        &self,
        key: &str,
        _: &EvaluationContext,
    ) -> EvaluationResult<ResolutionDetails<String>> {
        self.unsupported(key)
    }
    async fn resolve_struct_value(
        &self,
        key: &str,
        _: &EvaluationContext,
    ) -> EvaluationResult<ResolutionDetails<StructValue>> {
        self.unsupported(key)
    }
}

#[cfg(test)]
mod tests;
