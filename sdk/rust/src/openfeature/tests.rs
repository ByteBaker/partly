// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::Decision;
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
    async fn error<'a>(&self, _: &HookContext<'a>, _: &EvaluationError, _: Option<&'a HookHints>) {
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
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nETag: \"one\"\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
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
    assert!(
        flags
            .get_bool_value("adult", Some(&context), None)
            .await
            .unwrap()
    );
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
    assert!(
        !hooked
            .get_bool_value("adult", Some(&context), None)
            .await
            .unwrap()
    );
    let calls = Arc::new(Mutex::new(vec![]));
    let hooked = hooked.with_hook(AgeHook(calls.clone()));
    assert!(
        hooked
            .get_bool_value("adult", Some(&context), None)
            .await
            .unwrap()
    );
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
