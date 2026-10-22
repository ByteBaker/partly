// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

"use strict";
const { OpenFeature } = require("@openfeature/server-sdk");
const { Client } = require("partly-sdk");
const { PartlyProvider } = require("partly-sdk/openfeature");

const CONTROL_PLANE_URL = "http://127.0.0.1:8080";
const ENVIRONMENT = "development";
const APPLICATION_ID = "YOUR_APPLICATION_ID";
const API_KEY = "YOUR_API_KEY";

async function checkout(flags, context) {
  const result = await flags.getBooleanDetails("new_checkout", false, context);
  if (result.errorCode) throw new Error(result.errorCode);
  return result.value ? "Use the new checkout" : "Use the current checkout";
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
    await OpenFeature.setProviderAndWait("partly", new PartlyProvider(client));
    const flags = OpenFeature.getClient("partly");
    // Two example requests reuse the client; evaluation is local.
    for (const targetingKey of ["user-123", "user-456"]) {
      console.log(await checkout(flags, { targetingKey }));
    }
  } finally {
    try {
      await OpenFeature.close();
    } finally {
      await client.close(); // Application shutdown.
    }
  }
}

main().catch(error => {
  console.error("Partly:", error.message);
  process.exitCode = 1;
});
