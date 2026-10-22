// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use open_feature::{EvaluationContext, EvaluationResult, OpenFeature};
use partly::{openfeature::PartlyProvider, Client};
use std::sync::Arc;

const CONTROL_PLANE_URL: &str = "http://127.0.0.1:8080";
const ENVIRONMENT: &str = "development";
const APPLICATION_ID: &str = "YOUR_APPLICATION_ID";
const API_KEY: &str = "YOUR_API_KEY";

async fn checkout(
    flags: &open_feature::Client,
    context: &EvaluationContext,
) -> EvaluationResult<&'static str> {
    Ok(
        if flags
            .get_bool_value("new_checkout", Some(context), None)
            .await?
        {
            "Use the new checkout"
        } else {
            "Use the current checkout"
        },
    )
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Arc::new(
        Client::builder()
            .url(CONTROL_PLANE_URL)
            .environment(ENVIRONMENT)
            .application_id(APPLICATION_ID)
            .secret_key(API_KEY)
            .build()?,
    );
    let mut api = OpenFeature::default();
    let result = async {
        // Application startup, before accepting requests.
        client.init().await?;
        api.set_provider(PartlyProvider::new(client.clone())).await;
        let flags = api.create_client();
        // Two example requests reuse the client; evaluation is local.
        for user_id in ["user-123", "user-456"] {
            let context = EvaluationContext::default().with_targeting_key(user_id);
            // Upstream evaluation errors do not implement std::error::Error.
            let message = checkout(&flags, &context)
                .await
                .map_err(|error| format!("{error:?}"))?;
            println!("{message}");
        }
        Ok::<(), Box<dyn std::error::Error>>(())
    }
    .await;
    api.shutdown().await;
    client.close().await?; // Application shutdown.
    result // Let the application handle initialization or evaluation failures.
}
