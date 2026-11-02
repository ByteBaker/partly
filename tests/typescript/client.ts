// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

import { Client, Context, Decision, NamedTypeError, PartlyError, ResultTypeError } from "../../sdk/node";

async function lifecycle(): Promise<void> {
  const client = Client.builder().url("https://partly.example.com").environment("development").applicationId("app_test").secretKey("key").build();
  try {
    await client.init();
    const context: Context = { id: "user-1", age: 21, verified: true };
    const decision: Decision = client.evaluate("checkout", context);
    const title: string = client.evaluateString("title", context);
    const count: number = client.evaluateInteger("count", context);
    const ratio: number = client.evaluateFloat("ratio", context);
    const settings: Record<string, string | number | boolean> = client.evaluateObject("settings", context);
    const named: string | Record<string, string | number | boolean> = client.evaluateNamed("settings", "Settings", context);
    const typeName: string | undefined = client.resolve("settings", "object", context).named_type;
    const mismatch: ResultTypeError = new NamedTypeError("Wrong result type");
    const expected: string = mismatch.expected_type;
    const changed: boolean = await client.refresh();
    const error: PartlyError | null = client.lastRefreshError;
    void [decision, title, count, ratio, settings, named, typeName, expected, changed, error];
    // @ts-expect-error Context values must be flat primitives.
    client.evaluate("checkout", { account: { id: "nested" } });
    // @ts-expect-error Evaluation returns an explicit decision, not a boolean.
    const enabled: boolean = client.evaluate("checkout", context);
    void enabled;
    // @ts-expect-error Named evaluation requires the expected type name.
    client.evaluateNamed("settings");
    // @ts-expect-error A string getter cannot supply a numeric result.
    const numeric: number = client.evaluateString("title", context);
    void numeric;
  } finally {
    await client.close();
  }
}

void lifecycle; // Compile this contract without making network requests.
