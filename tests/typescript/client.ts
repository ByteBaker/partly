// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

import { Client, Context, Decision, PartlyError } from "../../sdk/node";

async function lifecycle(): Promise<void> {
  const client = Client.builder().url("https://partly.example.com").secretKey("key").build();
  try {
    await client.init();
    const context: Context = { id: "user-1", age: 21, verified: true };
    const decision: Decision = client.evaluate("checkout", context);
    const changed: boolean = await client.refresh();
    const error: PartlyError | null = client.lastRefreshError;
    void [decision, changed, error];
    // @ts-expect-error Context values must be flat primitives.
    client.evaluate("checkout", { account: { id: "nested" } });
    // @ts-expect-error Evaluation returns an explicit decision, not a boolean.
    const enabled: boolean = client.evaluate("checkout", context);
    void enabled;
  } finally {
    await client.close();
  }
}

void lifecycle; // Compile this contract without making network requests.
