// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

use partly::{Client, Context, Decision, Error};

const CONTROL_PLANE_URL: &str = "http://127.0.0.1:8080";
const API_KEY: &str = "YOUR_API_KEY";

#[tokio::main]
async fn main() -> Result<(), Error> {
    let client = Client::builder()
        .url(CONTROL_PLANE_URL)
        .secret_key(API_KEY)
        .build()?;
    let result = async {
        client.init().await?;
        let context = Context::from([("id".into(), "user-123".into())]);
        match client.evaluate("new_checkout", &context)? {
            Decision::Enabled => println!("Use the new checkout"),
            Decision::Disabled => println!("Use the current checkout"),
        }
        Ok(())
    }
    .await;
    client.close().await?;
    result // Let the application handle initialization or evaluation failures.
}
