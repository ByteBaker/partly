// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::models::{ErrorBody, SavedFlag};
use crate::store;
use partly::evaluator::Flag;
use serde_json::json;
use utoipa::{
    PartialSchema, ToSchema,
    openapi::{
        Content, ObjectBuilder, OpenApi, Ref, RefOr, Required, Schema, Type,
        header::Header,
        path::{ParameterBuilder, ParameterIn},
        response::ResponseBuilder,
        security::{ApiKey, ApiKeyValue, Http, HttpAuthScheme, SecurityScheme},
    },
};

// Creation and update share a parser, but only creation requires a non-null password.
pub struct CreateUserInput;

impl PartialSchema for CreateUserInput {
    fn schema() -> RefOr<Schema> {
        let mut schema =
            serde_json::to_value(store::UserInput::schema()).expect("User schema serializes");
        schema["required"]
            .as_array_mut()
            .expect("Required account fields")
            .push(json!("password"));
        schema["properties"]["password"]["type"] = json!("string");
        serde_json::from_value(schema).expect("Create user schema is valid")
    }
}

impl ToSchema for CreateUserInput {
    fn schemas(schemas: &mut Vec<(String, RefOr<Schema>)>) {
        store::UserInput::schemas(schemas);
    }
}

// Flattening a closed Flag with allOf would incorrectly reject the added key.
impl PartialSchema for SavedFlag {
    fn schema() -> RefOr<Schema> {
        let mut schema = Flag::schema();
        let RefOr::T(Schema::OneOf(variants)) = &mut schema else {
            panic!("Tagged flag variants")
        };
        for variant in &mut variants.items {
            let RefOr::T(Schema::Object(object)) = variant else {
                panic!("Flag variant object")
            };
            object.max_properties = object.max_properties.map(|count| count + 1);
            object.properties.insert(
                "key".into(),
                ObjectBuilder::new()
                    .schema_type(Type::String)
                    .pattern(Some("^[a-z0-9_-]{1,64}$"))
                    .build()
                    .into(),
            );
            object.required.push("key".into());
        }
        schema
    }
}

impl ToSchema for SavedFlag {
    fn schemas(schemas: &mut Vec<(String, RefOr<Schema>)>) {
        Flag::schemas(schemas);
    }
}

#[derive(utoipa::OpenApi)]
#[openapi(
    info(title = "Partly HTTP API", description = "Dashboard administration and SDK policy/usage transport. Evaluation runs locally in SDKs. JSON request bodies are limited to 16 KiB; policies to 1 MiB. Sessions expire after 12 hours or server restart. Cross-site API requests are rejected; HTTPS adds Secure to session cookies. Host and Origin checks use the actual listener, never forwarded headers."),
    components(schemas(ErrorBody, partly::evaluator::Condition)),
    tags(
        (name = "Session", description = "Setup and dashboard authentication"),
        (name = "Types", description = "Immutable named result schemas. Viewer reads; Manager or higher creates and deletes unused types."),
        (name = "Flags", description = "Viewer reads; Editor or higher writes"),
        (name = "Applications", description = "Viewer reads; Manager or higher writes. Only managers and administrators receive key secrets."),
        (name = "Users", description = "Instance administrator only"),
        (name = "Audits", description = "Environment-scoped, read-only history; Viewer or higher"),
        (name = "SDK", description = "Application-scoped API Keys. Cannot perform dashboard administration.")
    )
)]
pub(super) struct Contract;

pub(super) fn finish(document: &mut OpenApi) {
    let components = document.components.as_mut().expect("API components");
    components.add_security_scheme(
        "session",
        SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new("partly_session"))),
    );
    components.add_security_scheme(
        "api_key",
        SecurityScheme::Http(Http::new(HttpAuthScheme::Bearer)),
    );

    // These headers and failures come from shared middleware, rather than individual handlers.
    for (path, item) in &mut document.paths.paths {
        for (write, operation) in [
            (false, &mut item.get),
            (true, &mut item.post),
            (true, &mut item.put),
            (true, &mut item.delete),
        ] {
            let Some(operation) = operation else { continue };
            let sdk = path == "/api/policy" || path == "/api/usage";
            for (status, description) in [
                ("408", "Request body did not finish within 10 seconds"),
                ("413", "Request body exceeds 16 KiB"),
                ("503", "Too many active requests; Retry-After: 1"),
            ] {
                let mut response = ResponseBuilder::new()
                    .description(description)
                    .content(
                        "application/json",
                        Content::new(Some(Ref::from_schema_name("ErrorBody"))),
                    )
                    .build();
                if status == "503" {
                    response
                        .headers
                        .insert("Retry-After".into(), Header::new(String::schema()).into());
                }
                operation
                    .responses
                    .responses
                    .insert(status.into(), response.into());
            }
            let parameters = operation.parameters.get_or_insert_default();
            if write && !sdk {
                parameters.push(ParameterBuilder::new().name("X-Partly-Request").parameter_in(ParameterIn::Header)
                    .required(Required::True).schema(Some(ObjectBuilder::new().schema_type(Type::String).enum_values(Some(["dashboard"])))).example(Some(json!("dashboard")))
                    .description(Some("Must equal dashboard for every dashboard write, including setup/login/logout.")).build().into());
            }
            if sdk {
                for (name, description) in [
                    (
                        "X-Partly-Environment",
                        "Exact environment bound to the API Key",
                    ),
                    (
                        "X-Partly-Application",
                        "Exact application ID bound to the API Key",
                    ),
                ] {
                    parameters.push(
                        ParameterBuilder::new()
                            .name(name)
                            .parameter_in(ParameterIn::Header)
                            .required(Required::True)
                            .schema(Some(String::schema()))
                            .description(Some(description))
                            .build()
                            .into(),
                    );
                }
            }
            for (status, description) in [
                ("400", "Invalid request or missing/invalid Host header"),
                (
                    "403",
                    "Insufficient permissions, missing dashboard header, or rejected Origin/cross-site request",
                ),
                ("421", "Hostname is not allowed"),
                ("500", "Internal failure; details are omitted"),
            ] {
                operation
                    .responses
                    .responses
                    .entry(status.into())
                    .or_insert_with(|| {
                        ResponseBuilder::new()
                            .description(description)
                            .content(
                                "application/json",
                                Content::new(Some(Ref::from_schema_name("ErrorBody"))),
                            )
                            .build()
                            .into()
                    });
            }
            if operation
                .security
                .as_ref()
                .is_some_and(|requirements| !requirements.is_empty())
            {
                operation
                    .responses
                    .responses
                    .entry("401".into())
                    .or_insert_with(|| {
                        ResponseBuilder::new()
                            .description("Authentication required or credentials invalid")
                            .content(
                                "application/json",
                                Content::new(Some(Ref::from_schema_name("ErrorBody"))),
                            )
                            .build()
                            .into()
                    });
            }
            for response in operation.responses.responses.values_mut() {
                if let RefOr::T(response) = response {
                    let mut header = Header::new(String::schema());
                    header.example = Some(json!("no-store"));
                    response
                        .headers
                        .insert("Cache-Control".into(), header.into());
                }
            }
        }
    }
}
