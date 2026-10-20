// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use partly::{Client, Context, Decision, Error};

const CONTROL_PLANE_URL: &str = "http://127.0.0.1:8080";
const API_KEY: &str = "YOUR_API_KEY";

fn checkout(client: &Client, context: &Context) -> Result<&'static str, Error> {
    Ok(match client.evaluate("new_checkout", context)? {
        Decision::Enabled => "Use the new checkout",
        Decision::Disabled => "Use the current checkout",
    })
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    let client = Client::builder()
        .url(CONTROL_PLANE_URL)
        .secret_key(API_KEY)
        .build()?;
    let result = async {
        // Application startup, before accepting requests.
        client.init().await?;
        // Two example requests reuse the initialized client and its policy.
        for user_id in ["user-123", "user-456"] {
            let context = Context::from([("id".into(), user_id.into())]);
            println!("{}", checkout(&client, &context)?);
        }
        Ok(())
    }
    .await;
    client.close().await?; // Application shutdown.
    result // Let the application handle initialization or evaluation failures.
}
