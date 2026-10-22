// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

"use strict";
const { test } = require('node:test');
const assert = require('node:assert/strict');
const http = require('node:http');
const { once } = require('node:events');
const { OpenFeature, ProviderEvents, ProviderStatus } = require('@openfeature/server-sdk');
const { Client, KeyRevokedError } = require('..');
const { PartlyProvider } = require('../openfeature');

test('official OpenFeature SDK: local decisions, context, defaults, refresh and revoked keys', async () => {
  let requests = 0; let revoked = false;
  const policy = { schema_version: 1, project: 'default', environment: 'development', application_id: 'app_test', revision: 1,
    flags: { off: { mode: 'disabled' }, rollout: { mode: 'percentage', percentage: 50 },
      adult: { mode: 'custom', action: 'enable', groups: [[{ k: 'age', op: 'ge', v: 18 }]] } } };
  const server = http.createServer((_request, response) => {
    requests++;
    response.writeHead(revoked ? 401 : 200, revoked ? { 'X-Partly-Error': 'key_revoked' } : { ETag: `"${policy.revision}"` });
    response.end(revoked ? '' : JSON.stringify(policy));
  });
  server.listen(0, '127.0.0.1'); await once(server, 'listening');
  const native = Client.builder().url(`http://127.0.0.1:${server.address().port}`).environment('development').applicationId('app_test').secretKey('key').pollInterval(3600).build();
  const other = Client.builder().url(`http://127.0.0.1:${server.address().port}`).environment('development').applicationId('app_test').secretKey('key').pollInterval(3600).build();
  const provider = new PartlyProvider(native);
  try {
    const lifecycle = [];
    const flags = OpenFeature.getClient('partly-test');
    flags.addHandler(ProviderEvents.Ready, () => lifecycle.push('ready'));
    flags.addHandler(ProviderEvents.Error, () => lifecycle.push('error'));
    await assert.rejects(OpenFeature.setProviderAndWait('partly-test', new PartlyProvider(native)), { code: 'PROVIDER_NOT_READY' });
    assert.equal(flags.providerStatus, ProviderStatus.ERROR);
    assert.deepEqual(lifecycle, ['error']);
    assert.equal(requests, 0);
    await native.init();
    await OpenFeature.setProviderAndWait('partly-test', provider);
    assert.equal(flags.providerStatus, ProviderStatus.READY);
    assert.deepEqual(lifecycle, ['error', 'ready']);
    assert.equal(OpenFeature.getProviderMetadata('partly-test').name, 'Partly');
    const off = await flags.getBooleanDetails('off', true);
    assert.equal(off.value, false); assert.equal(off.errorCode, undefined); assert.equal(off.variant, 'disabled');
    const missing = await flags.getBooleanDetails('missing', true);
    assert.equal(missing.value, true); assert.equal(missing.errorCode, 'FLAG_NOT_FOUND');
    for (const [method, fallback] of [['getStringDetails', 'safe'], ['getNumberDetails', 5], ['getObjectDetails', { safe: true }]]) {
      const result = await flags[method]('off', fallback);
      assert.deepEqual(result.value, fallback); assert.equal(result.errorCode, 'TYPE_MISMATCH');
      assert.equal((await flags[method]('missing', fallback)).errorCode, 'FLAG_NOT_FOUND');
    }
    assert.equal((await flags.getBooleanDetails('adult', false, { age: 'secret' })).errorCode, 'INVALID_CONTEXT');
    assert.equal(await flags.getBooleanValue('adult', false, { age: 21, extra: {} }), true);
    assert.equal(await flags.getBooleanValue('rollout', false, { targetingKey: 'user-42', id: 'ignored' }), native.evaluate('rollout', { id: 'user-42' }) === 'enabled');
    assert.equal(requests, 1);
    OpenFeature.setContext({ age: 17 });
    const hooked = OpenFeature.getClient('partly-test');
    assert.equal(await hooked.getBooleanValue('adult', true), false);
    hooked.setContext({ age: 21 });
    assert.equal(await hooked.getBooleanValue('adult', false), true);
    assert.equal(await hooked.getBooleanValue('adult', true, { age: 17 }), false);
    const hooks = [];
    hooked.addHooks({
      before: context => { hooks.push('before'); assert.equal(context.context.age, 17); return { age: 23 }; },
      after: (_context, details) => { hooks.push('after'); assert.equal(details.value, true); },
      error: () => hooks.push('error'),
      finally: () => hooks.push('finally'),
    });
    assert.equal(await hooked.getBooleanValue('adult', false, { age: 17, date: new Date(), nested: [1] }), true);
    assert.deepEqual(hooks, ['before', 'after', 'finally']);
    hooks.length = 0;
    assert.equal((await hooked.getBooleanDetails('missing', false, { age: 17 })).errorCode, 'FLAG_NOT_FOUND');
    assert.deepEqual(hooks, ['before', 'error', 'finally']);
    await other.init();
    await OpenFeature.setProviderAndWait('isolated', new PartlyProvider(other));
    policy.flags.off = { mode: 'enabled' }; policy.revision++;
    await native.refresh(); assert.equal(await flags.getBooleanValue('off', false), true);
    assert.equal(await OpenFeature.getClient('isolated').getBooleanValue('off', true), false);
    await OpenFeature.setProviderAndWait('isolated', new PartlyProvider(native));
    assert.equal(await OpenFeature.getClient('isolated').getBooleanValue('off', false), true);
    assert.equal(other.hasFlag('off'), true);
    revoked = true; await assert.rejects(native.refresh(), KeyRevokedError);
    assert.equal((await flags.getBooleanDetails('off', false)).errorCode, 'PROVIDER_FATAL');
    await assert.rejects(provider.initialize(), { code: 'PROVIDER_FATAL' });
    await assert.rejects(provider.resolveBooleanEvaluation('off', false), { code: 'PROVIDER_NOT_READY' });
    await provider.onClose();
    await assert.rejects(provider.resolveBooleanEvaluation('off', false), { code: 'PROVIDER_NOT_READY' });
  } finally {
    await OpenFeature.close(); await native.close(); await other.close(); server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  }
});
