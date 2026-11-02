// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

//! Borrow an initialized client. The application owns its async initialization and cleanup.

use crate::{Client, Context, Error, Resolution};
use async_trait::async_trait;
use open_feature::{
    EvaluationContext, EvaluationContextFieldValue as Field, EvaluationError,
    EvaluationErrorCode as Code, EvaluationReason, EvaluationResult, FlagMetadata, StructValue,
    Value,
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

    fn resolve(
        &self,
        key: &str,
        kind: &str,
        context: &EvaluationContext,
    ) -> EvaluationResult<Resolution> {
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
        self.client
            .resolve(key, &attributes, kind, None)
            .map_err(map_error)
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
        Error::Evaluation(
            crate::EvaluationError::ResultType { .. } | crate::EvaluationError::NamedType { .. },
        ) => Code::TypeMismatch,
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
        let result = self.resolve(key, "boolean", context)?;
        let value =
            serde_json::from_value(result.value.clone()).map_err(|_| error(Code::TypeMismatch))?;
        Ok(details(result, value))
    }
    async fn resolve_int_value(
        &self,
        key: &str,
        context: &EvaluationContext,
    ) -> EvaluationResult<ResolutionDetails<i64>> {
        let result = self.resolve(key, "integer", context)?;
        let value =
            serde_json::from_value(result.value.clone()).map_err(|_| error(Code::TypeMismatch))?;
        Ok(details(result, value))
    }
    async fn resolve_float_value(
        &self,
        key: &str,
        context: &EvaluationContext,
    ) -> EvaluationResult<ResolutionDetails<f64>> {
        let result = self.resolve(key, "float", context)?;
        let value =
            serde_json::from_value(result.value.clone()).map_err(|_| error(Code::TypeMismatch))?;
        Ok(details(result, value))
    }
    async fn resolve_string_value(
        &self,
        key: &str,
        context: &EvaluationContext,
    ) -> EvaluationResult<ResolutionDetails<String>> {
        let result = self.resolve(key, "string", context)?;
        let value =
            serde_json::from_value(result.value.clone()).map_err(|_| error(Code::TypeMismatch))?;
        Ok(details(result, value))
    }
    async fn resolve_struct_value(
        &self,
        key: &str,
        context: &EvaluationContext,
    ) -> EvaluationResult<ResolutionDetails<StructValue>> {
        let result = self.resolve(key, "object", context)?;
        let fields = result
            .value
            .as_object()
            .ok_or_else(|| error(Code::TypeMismatch))?
            .iter()
            .map(|(key, value)| {
                let value = match value {
                    serde_json::Value::Bool(v) => Value::Bool(*v),
                    serde_json::Value::String(v) => Value::String(v.clone()),
                    serde_json::Value::Number(v) if v.is_i64() => Value::Int(v.as_i64().unwrap()),
                    serde_json::Value::Number(v) => Value::Float(v.as_f64().unwrap()),
                    _ => return Err(error(Code::TypeMismatch)),
                };
                Ok((key.clone(), value))
            })
            .collect::<EvaluationResult<_>>()?;
        Ok(details(result, StructValue { fields }))
    }
}

fn details<T>(result: Resolution, value: T) -> ResolutionDetails<T> {
    ResolutionDetails {
        value,
        variant: Some(result.variant),
        reason: Some(EvaluationReason::Cached),
        flag_metadata: result
            .named_type
            .map(|name| FlagMetadata::default().with_value("partly.result_type", name)),
    }
}

#[cfg(test)]
mod tests;
