// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

//! Borrow an initialized client. The application owns its async initialization and cleanup.

use crate::{Client, Context, Decision, Error};
use async_trait::async_trait;
use open_feature::{
    provider::{FeatureProvider, ProviderMetadata, ProviderStatus, ResolutionDetails},
    EvaluationContext, EvaluationContextFieldValue as Field, EvaluationError,
    EvaluationErrorCode as Code, EvaluationReason, EvaluationResult, StructValue,
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
mod tests {
    use super::*;
    use open_feature::{EvaluationDetails, Hook, HookContext, HookHints, OpenFeature, Value};
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::Mutex,
        thread,
    };

    struct AgeHook(Arc<Mutex<Vec<&'static str>>>);

    #[async_trait]
    impl Hook for AgeHook {
        async fn before<'a>(
            &self,
            context: &HookContext<'a>,
            _: Option<&'a HookHints>,
        ) -> EvaluationResult<Option<EvaluationContext>> {
            self.0.lock().unwrap().push("before");
            assert_eq!(context.provider_metadata.name, "Partly");
            assert_eq!(
                context.evaluation_context.custom_fields["age"],
                Field::Int(17)
            );
            Ok(Some(
                EvaluationContext::default().with_custom_field("age", 23),
            ))
        }
        async fn after<'a>(
            &self,
            _: &HookContext<'a>,
            details: &EvaluationDetails<Value>,
            _: Option<&'a HookHints>,
        ) -> EvaluationResult<()> {
            self.0.lock().unwrap().push("after");
            assert_eq!(details.value, Value::Bool(true));
            Ok(())
        }
        async fn error<'a>(
            &self,
            _: &HookContext<'a>,
            _: &EvaluationError,
            _: Option<&'a HookHints>,
        ) {
            self.0.lock().unwrap().push("error");
        }
        async fn finally<'a>(
            &self,
            _: &HookContext<'a>,
            _: &EvaluationDetails<Value>,
            _: Option<&'a HookHints>,
        ) {
            self.0.lock().unwrap().push("finally");
        }
    }

    #[tokio::test]
    async fn official_sdk_uses_local_policy_and_explicit_errors() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0; 4096];
            assert!(stream.read(&mut request).unwrap() > 0);
            let body = r#"{"schema_version":1,"project":"default","environment":"development","application_id":"app_test","revision":1,"flags":{"off":{"mode":"disabled"},"rollout":{"mode":"percentage","percentage":50},"adult":{"mode":"custom","action":"enable","groups":[[{"k":"age","op":"ge","v":18}]]}}}"#;
            write!(stream, "HTTP/1.1 200 OK\r\nETag: \"one\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        });
        let native = Arc::new(
            Client::builder()
                .url(url)
                .environment("development")
                .application_id("app_test")
                .secret_key("key")
                .build()
                .unwrap(),
        );
        let mut provider = PartlyProvider::new(native.clone());
        provider.initialize(&EvaluationContext::default()).await;
        assert_eq!(provider.status(), ProviderStatus::Error);
        native.init().await.unwrap();
        server.join().unwrap();
        let mut api = OpenFeature::default();
        api.set_provider(PartlyProvider::new(native.clone())).await;
        let flags = api.create_client();
        let off = flags.get_bool_details("off", None, None).await.unwrap();
        assert!(!off.value);
        assert_eq!(off.variant.as_deref(), Some("disabled"));
        assert_eq!(off.reason, Some(EvaluationReason::Cached));
        for key in ["off", "missing"] {
            let expected = if key == "off" {
                Code::TypeMismatch
            } else {
                Code::FlagNotFound
            };
            assert_eq!(
                flags
                    .get_int_details(key, None, None)
                    .await
                    .unwrap_err()
                    .code,
                expected
            );
            assert_eq!(
                flags
                    .get_float_details(key, None, None)
                    .await
                    .unwrap_err()
                    .code,
                expected
            );
            assert_eq!(
                flags
                    .get_struct_details::<StructValue>(key, None, None)
                    .await
                    .unwrap_err()
                    .code,
                expected
            );
            assert_eq!(
                flags
                    .get_string_details(key, None, None)
                    .await
                    .unwrap_err()
                    .code,
                expected
            );
        }
        assert_eq!(
            flags
                .get_bool_details("missing", None, None)
                .await
                .unwrap_err()
                .code,
            Code::FlagNotFound
        );
        assert_eq!(
            flags
                .get_string_details("off", None, None)
                .await
                .unwrap_err()
                .code,
            Code::TypeMismatch
        );
        assert_eq!(
            flags
                .get_bool_details("adult", None, None)
                .await
                .unwrap_err()
                .code,
            Code::InvalidContext
        );
        let context = EvaluationContext::default()
            .with_custom_field("age", 21)
            .with_custom_field("extra", Field::new_struct(()));
        assert!(flags
            .get_bool_value("adult", Some(&context), None)
            .await
            .unwrap());
        let context = EvaluationContext::default()
            .with_targeting_key("user-42")
            .with_custom_field("id", "ignored");
        assert_eq!(
            flags
                .get_bool_value("rollout", Some(&context), None)
                .await
                .unwrap(),
            native
                .evaluate("rollout", &Context::from([("id".into(), "user-42".into())]))
                .unwrap()
                == Decision::Enabled
        );
        api.set_evaluation_context(EvaluationContext::default().with_custom_field("age", 17))
            .await;
        let mut hooked = api.create_client();
        assert!(!hooked.get_bool_value("adult", None, None).await.unwrap());
        hooked.set_evaluation_context(EvaluationContext::default().with_custom_field("age", 21));
        assert!(hooked.get_bool_value("adult", None, None).await.unwrap());
        let context = EvaluationContext::default().with_custom_field("age", 17);
        assert!(!hooked
            .get_bool_value("adult", Some(&context), None)
            .await
            .unwrap());
        let calls = Arc::new(Mutex::new(vec![]));
        let hooked = hooked.with_hook(AgeHook(calls.clone()));
        assert!(hooked
            .get_bool_value("adult", Some(&context), None)
            .await
            .unwrap());
        assert_eq!(*calls.lock().unwrap(), ["before", "after", "finally"]);
        calls.lock().unwrap().clear();
        assert_eq!(
            hooked
                .get_bool_value("missing", Some(&context), None)
                .await
                .unwrap_err()
                .code,
            Code::FlagNotFound
        );
        assert_eq!(*calls.lock().unwrap(), ["before", "error", "finally"]);
        let uninitialized = Arc::new(
            Client::builder()
                .url("http://127.0.0.1:1")
                .environment("development")
                .application_id("app_other")
                .secret_key("key")
                .build()
                .unwrap(),
        );
        api.set_named_provider("isolated", PartlyProvider::new(uninitialized.clone()))
            .await;
        let isolated = api.create_named_client("isolated");
        assert!(isolated.get_bool_value("off", None, None).await.is_err());
        assert!(!flags.get_bool_value("off", None, None).await.unwrap());
        api.set_named_provider("isolated", PartlyProvider::new(native.clone()))
            .await;
        assert!(!isolated.get_bool_value("off", None, None).await.unwrap());
        uninitialized.close().await.unwrap();
        assert!(native.refresh().await.is_err());
        assert!(!flags.get_bool_value("off", None, None).await.unwrap());
        native
            .inner
            .revoked
            .store(true, std::sync::atomic::Ordering::Release);
        assert_eq!(
            flags
                .get_bool_value("off", None, None)
                .await
                .unwrap_err()
                .code
                .to_string(),
            "PROVIDER_FATAL"
        );
        api.shutdown().await;
        native.close().await.unwrap();
    }
}
