// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

import { OpenFeature } from '@openfeature/server-sdk';
import { Client } from '../../sdk/node';
import { PartlyProvider } from '../../sdk/node/openfeature';

async function example(client: Client): Promise<boolean> {
  await client.init();
  await OpenFeature.setProviderAndWait(new PartlyProvider(client));
  const flags = OpenFeature.getClient();
  try {
    const result = await flags.getBooleanDetails('checkout', false, { targetingKey: 'user-1' });
    return result.value;
  } finally {
    await OpenFeature.close();
    await client.close();
  }
}
void example;
