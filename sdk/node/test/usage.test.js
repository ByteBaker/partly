// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

"use strict";
const { test } = require("node:test");
const assert = require("node:assert/strict");
const http = require("node:http");
const { once } = require("node:events");
const { setImmediate: tick } = require("node:timers/promises");
const { Client, Decision, FlagNotFoundError, InvalidContextError, KeyRevokedError, RefreshError } = require("..");

async function fixture(t) {
  const state = { now: 0, status: 204, usageGate: Promise.resolve(), refreshGate: Promise.resolve(), revoked: false };
  t.mock.method(performance, "now", () => state.now);
  const policy = { schema_version: 1, application_id: "app_test", project: "default", environment: "development", revision: 1,
    flags: { ...Object.fromEntries(Array.from({ length: 300 }, (_, i) => [`f${i}`, { mode: "enabled" }])),
      off: { mode: "disabled" }, context: { mode: "percentage", percentage: 50 } } };
  const requests = [];
  const completed = [];
  const request = http.request;
  t.mock.method(http, "request", (...args) => {
    const req = request(...args);
    if (args[0].pathname === "/api/usage") completed.push(new Promise(resolve => req.once("close", resolve)));
    return req;
  });
  const server = http.createServer(async (request, response) => {
    if (request.url === "/api/usage") {
      const chunks = [];
      for await (const chunk of request) chunks.push(chunk);
      requests.push({ headers: request.headers, method: request.method, body: JSON.parse(Buffer.concat(chunks)) });
      await state.usageGate;
      response.writeHead(state.status, state.revoked ? { "X-Partly-Error": "key_revoked" } : {});
      response.end();
    } else {
      await state.refreshGate;
      response.writeHead(state.refreshRevoked ? 401 : 200, state.refreshRevoked ? { "X-Partly-Error": "key_revoked" } : { ETag: '"one"' });
      response.end(state.refreshRevoked ? undefined : JSON.stringify(policy));
    }
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const client = Client.builder().url(`http://127.0.0.1:${server.address().port}`).environment("development").applicationId("app_test")
    .secretKey("test-key").pollInterval(3600).timeout(0.5).build();
  t.after(async () => {
    await client.close();
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  });
  await client.init();
  return { client, state, policy, requests, completed };
}

async function received(requests, count) {
  const deadline = Date.now() + 1000;
  while (requests.length < count && Date.now() < deadline) await tick();
  assert.equal(requests.length, count);
}

async function finishReport(completed, index) {
  await Promise.resolve();
  assert.ok(completed[index]);
  await completed[index];
}

test("usage is lazy, distinct, scoped, and includes disabled and context failures", async t => {
  const { client, state, requests, completed } = await fixture(t);
  assert.equal(client.hasFlag("f1"), true);
  assert.throws(() => client.evaluate("missing"), FlagNotFoundError);
  assert.equal(client.evaluate("off"), Decision.DISABLED);
  assert.throws(() => client.evaluate("context"), InvalidContextError);
  for (let i = 0; i < 200; i++) client.evaluate("off");
  state.now = 59999;
  client.evaluate("f0");
  assert.equal(completed.length, 0);
  state.now = 60000;
  await tick();
  assert.equal(completed.length, 0);
  client.evaluate("off");
  assert.equal(completed.length, 0);
  await finishReport(completed, 0);
  assert.equal(requests.length, 1);
  assert.deepEqual(requests[0].body, { flags: ["off", "context", "f0"] });
  assert.equal(requests[0].method, "POST");
  assert.equal(requests[0].headers.authorization, "Bearer test-key");
  assert.equal(requests[0].headers["x-partly-environment"], "development");
  assert.equal(requests[0].headers["x-partly-application"], "app_test");
  assert.equal(requests[0].headers["if-none-match"], undefined);
});

test("threshold triggers one request and preserves reevaluations during flight", async t => {
  const { client, state, requests, completed } = await fixture(t);
  const gate = Promise.withResolvers();
  state.usageGate = gate.promise;
  for (let i = 0; i < 127; i++) client.evaluate(`f${i}`);
  assert.equal(completed.length, 0);
  client.evaluate("f127");
  assert.equal(completed.length, 0);
  await received(requests, 1);
  for (let i = 0; i < 200; i++) client.evaluate(`f${i}`);
  assert.equal(completed.length, 1);
  gate.resolve();
  await finishReport(completed, 0);
  assert.deepEqual(requests[0].body.flags, Array.from({ length: 128 }, (_, i) => `f${i}`));
  client.evaluate("f0");
  await finishReport(completed, 1);
  assert.deepEqual(requests[1].body.flags, Array.from({ length: 128 }, (_, i) => `f${i}`));
  state.now = 60000;
  client.evaluate("f0");
  await finishReport(completed, 2);
  assert.deepEqual(requests[2].body.flags, [...Array.from({ length: 72 }, (_, i) => `f${128 + i}`), "f0"]);
});

test("overdue tails remain FIFO and do not wait another interval", async t => {
  const { client, state, requests, completed } = await fixture(t);
  const gate = Promise.withResolvers();
  state.usageGate = gate.promise;
  for (let i = 0; i < 128; i++) client.evaluate(`f${i}`);
  await received(requests, 1);
  for (let i = 128; i < 300; i++) client.evaluate(`f${i}`);
  state.now = 60000;
  gate.resolve();
  await finishReport(completed, 0);
  client.evaluate("f0");
  await finishReport(completed, 1);
  client.evaluate("f0");
  await finishReport(completed, 2);
  assert.deepEqual(requests[1].body.flags, Array.from({ length: 128 }, (_, i) => `f${128 + i}`));
  assert.deepEqual(requests[2].body.flags, [...Array.from({ length: 44 }, (_, i) => `f${256 + i}`), "f0"]);
});

for (const status of [400, 401, 500, 302]) {
  test(`ordinary usage HTTP ${status} keeps policy and enforces a cooldown even when full`, async t => {
    const { client, state, requests, completed, policy } = await fixture(t);
    policy.schema_version = 999;
    await assert.rejects(client.refresh());
    const refreshError = client.lastRefreshError;
    state.status = status;
    for (let i = 0; i < 128; i++) client.evaluate(`f${i}`);
    await finishReport(completed, 0);
    assert.equal(client.lastRefreshError, refreshError);
    state.now = 59999;
    assert.equal(client.evaluate("f0"), Decision.ENABLED);
    assert.equal(completed.length, 1);
    state.now = 60000;
    client.evaluate("f0");
    await finishReport(completed, 1);
    assert.equal(requests.length, 2);
  });
}

test("accepted policies prune pending names and failed batches cannot restore them", async t => {
  const { client, state, requests, completed, policy } = await fixture(t);
  const gate = Promise.withResolvers();
  state.usageGate = gate.promise;
  state.status = 500;
  state.now = 60000;
  client.evaluate("f0");
  await received(requests, 1);
  client.evaluate("f1");
  client.evaluate("f2");
  policy.flags = { f1: { mode: "enabled" } };
  await client.refresh();
  gate.resolve();
  await finishReport(completed, 0);
  state.status = 204;
  state.now = 120000;
  client.evaluate("f1");
  await finishReport(completed, 1);
  assert.deepEqual(requests[1].body, { flags: ["f1"] });
});

test("usage revocation is terminal even with concurrent refresh", async t => {
  const { client, state, requests, completed } = await fixture(t);
  const usageGate = Promise.withResolvers();
  const refreshGate = Promise.withResolvers();
  state.usageGate = usageGate.promise;
  state.refreshGate = refreshGate.promise;
  state.status = 401;
  state.revoked = true;
  state.now = 60000;
  client.evaluate("f0");
  await received(requests, 1);
  const refresh = assert.rejects(client.refresh(), KeyRevokedError);
  await tick();
  usageGate.resolve();
  await finishReport(completed, 0);
  refreshGate.resolve();
  await refresh;
  assert.ok(client.lastRefreshError instanceof KeyRevokedError);
  assert.throws(() => client.evaluate("f0"), KeyRevokedError);
  await assert.rejects(client.init(), KeyRevokedError);
  await assert.rejects(client.refresh(), KeyRevokedError);
  await client.close();
  assert.throws(() => client.evaluate("f0"), KeyRevokedError);
  assert.equal(completed.length, 1);
});

test("close cancels reporting and permits cached evaluation without new reports", async t => {
  const { client, state, requests, completed } = await fixture(t);
  const gate = Promise.withResolvers();
  state.usageGate = gate.promise;
  state.now = 60000;
  client.evaluate("f0");
  await received(requests, 1);
  await client.close();
  gate.resolve();
  await finishReport(completed, 0);
  for (let i = 0; i < 200; i++) assert.equal(client.evaluate(`f${i}`), Decision.ENABLED);
  assert.equal(completed.length, 1);
  await client.close();
});

test("usage timeout preserves local decisions and refresh status", async t => {
  const { client, state, requests, completed } = await fixture(t);
  const gate = Promise.withResolvers();
  state.usageGate = gate.promise;
  state.now = 60000;
  client.evaluate("f0");
  await finishReport(completed, 0);
  gate.resolve();
  assert.equal(client.lastRefreshError, null);
  assert.equal(client.evaluate("f0"), Decision.ENABLED);
  assert.equal(requests.length, 1);
  assert.equal(completed.length, 1);
  assert.equal(client.lastRefreshError instanceof RefreshError, false);
});

test("refresh revocation cancels an in-flight usage request", async t => {
  const { client, state, requests, completed } = await fixture(t);
  const gate = Promise.withResolvers();
  state.usageGate = gate.promise;
  state.now = 60000;
  client.evaluate("f0");
  await received(requests, 1);
  state.refreshRevoked = true;
  await assert.rejects(client.refresh(), KeyRevokedError);
  await finishReport(completed, 0);
  gate.resolve();
  assert.throws(() => client.evaluate("f0"), KeyRevokedError);
  assert.equal(completed.length, 1);
});

test("close before the scheduled report starts makes no request", async t => {
  const { client, state, requests, completed } = await fixture(t);
  state.now = 60000;
  assert.equal(client.evaluate("f0"), Decision.ENABLED);
  assert.equal(completed.length, 0);
  await client.close();
  assert.equal(requests.length, 0);
  assert.equal(completed.length, 0);
  assert.equal(client.evaluate("f1"), Decision.ENABLED);
});
