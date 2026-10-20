// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

"use strict";
const { Client, Decision } = require("../../sdk/node");

const CONTROL_PLANE_URL = "http://127.0.0.1:8080";
const API_KEY = "YOUR_API_KEY";

async function main() {
  const client = Client.builder().url(CONTROL_PLANE_URL).secretKey(API_KEY).build();
  try {
    await client.init();
    const decision = client.evaluate("new_checkout", { id: "user-123" });
    console.log(decision === Decision.ENABLED ? "Use the new checkout" : "Use the current checkout");
  } finally {
    await client.close();
  }
}

main().catch(error => {
  console.error("Partly:", error.message);
  process.exitCode = 1;
});
