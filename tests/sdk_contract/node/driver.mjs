// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

import { createRequire } from "node:module";
import { createInterface } from "node:readline";

const { Client, ContextTypeError, Decision } = createRequire(import.meta.url)("../../../sdk/node");
const errors = {
  FlagNotFoundError: "flag_not_found",
  InvalidContextError: "invalid_context",
  ContextTypeError: "context_type",
  PolicyNotReadyError: "policy_not_ready",
  KeyRevokedError: "key_revoked", AuthenticationError: "authentication",
  InvalidPolicyError: "invalid_policy",
  RefreshError: "refresh",
  ClientClosedError: "client_closed",
};
let client;

try {
  for await (const line of createInterface({ input: process.stdin })) {
    const command = JSON.parse(line);
    let result = { ok: true };
    try {
      switch (command.op) {
        case "build": {
          const builder = Client.builder().url(command.url).environment(command.environment).applicationId(command.application_id).secretKey(command.key)
            .pollInterval(command.interval).timeout(command.timeout);
          if (command.ca_bundle) builder.caBundle(command.ca_bundle);
          client = builder.build();
          break;
        }
        case "init":
          await client.init();
          break;
        case "evaluate": {
          const decision = client.evaluate(command.flag, command.context);
          if (decision !== Decision.ENABLED && decision !== Decision.DISABLED) {
            throw new Error("SDK returned an unknown decision");
          }
          result = { decision: decision === Decision.ENABLED ? "enabled" : "disabled" };
          break;
        }
        case "refresh":
          result = { updated: await client.refresh() };
          break;
        case "close":
          await client.close();
          break;
        default:
          throw new Error(`Unknown contract operation: ${command.op}`);
      }
    } catch (error) {
      if (!Object.hasOwn(errors, error.name)) throw error;
      result = { error: errors[error.name] };
      if (error instanceof ContextTypeError) {
        const { flag_key, k, op, expected_type, actual_type } = error;
        Object.assign(result, { flag_key, k, op, expected_type, actual_type });
      }
    }
    process.stdout.write(`${JSON.stringify(result)}\n`);
  }
} finally {
  if (client) await client.close();
}
