// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

"use strict";
const { Client, Decision } = require("partly-sdk");

const CONTROL_PLANE_URL = "http://127.0.0.1:8080";
const ENVIRONMENT = "development";
const APPLICATION_ID = "YOUR_APPLICATION_ID";
const API_KEY = "YOUR_API_KEY";

function checkout(client, context) {
  const decision = client.evaluate("new_checkout", context);
  return decision === Decision.ENABLED ? "Use the new checkout" : "Use the current checkout";
}

async function main() {
  const client = Client.builder()
    .url(CONTROL_PLANE_URL)
    .environment(ENVIRONMENT)
    .applicationId(APPLICATION_ID)
    .secretKey(API_KEY)
    .build();
  try {
    await client.init(); // Application startup, before accepting requests.
    // Two example requests reuse the initialized client and its policy.
    for (const id of ["user-123", "user-456"]) {
      console.log(checkout(client, { id }));
    }
  } finally {
    await client.close(); // Application shutdown.
  }
}

main().catch(error => {
  console.error("Partly:", error.message);
  process.exitCode = 1;
});
