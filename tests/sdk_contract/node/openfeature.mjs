// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

import { createRequire } from 'node:module';
import { createInterface } from 'node:readline';
const require = createRequire(import.meta.url);
const { OpenFeature } = require('@openfeature/server-sdk');
const { Client } = require('../../../sdk/node');
const { PartlyProvider } = require('../../../sdk/node/openfeature');
let native;
try {
  for await (const line of createInterface({ input: process.stdin })) {
    const command = JSON.parse(line);
    let result = { ok: true };
    try {
      switch (command.op) {
        case 'build':
          native = Client.builder().url(command.url).environment(command.environment)
            .applicationId(command.application_id).secretKey(command.key).pollInterval(3600).build();
          break;
        case 'init':
          await native.init();
          await OpenFeature.setProviderAndWait('contract', new PartlyProvider(native));
          break;
        case 'evaluate': {
          const details = await OpenFeature.getClient('contract').getBooleanDetails(command.flag, command.fallback ?? false, command.context);
          result = { value: details.value, error: details.errorCode ?? '', variant: details.variant, reason: details.reason };
          break;
        }
        case 'refresh': result = { updated: await native.refresh() }; break;
        case 'close': await OpenFeature.close(); await native.close(); break;
        default: throw new Error(`Unknown operation: ${command.op}`);
      }
    } catch (error) { result = { error: error.name }; }
    process.stdout.write(`${JSON.stringify(result)}\n`);
  }
} finally {
  await OpenFeature.close();
  await native?.close();
}
