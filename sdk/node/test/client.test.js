// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

"use strict";
const { test } = require("node:test");
const assert = require("node:assert/strict");
const http = require("node:http");
const { once } = require("node:events");
const { Client, Decision, ContextTypeError, InvalidContextError, FlagNotFoundError, ClientClosedError, PolicyNotReadyError } = require("..");

test("builder validates without contacting the server", async () => {
  for (const url of ["ftp://host", "http://u:p@host", "http://host/path", "http://host?", "http://host#x", "http://host:99999"]) {
    assert.throws(() => Client.builder().url(url).environment("development").applicationId("app_test").secretKey("key").build(), TypeError);
  }
  for (const timeout of [0, -1, NaN, Infinity, true, 2147484]) {
    assert.throws(() => Client.builder().url("http://localhost").environment("development").applicationId("app_test").secretKey("key").timeout(timeout).build(), TypeError);
  }
  assert.throws(() => Client.builder().url("http://localhost").environment("development").applicationId("app_test").secretKey("key\n").build(), TypeError);
  const client = Client.builder().url("http://127.0.0.1:1").environment("development").applicationId("app_test").secretKey("key").build();
  assert.throws(() => client.evaluate("flag"), PolicyNotReadyError);
  await client.close();
  await assert.rejects(client.init(), ClientClosedError);
});

test("concurrent init, native errors, and close during a download", async () => {
  let requests = 0;
  const policy = { schema_version: 1, application_id: "app_test", project: "default", environment: "development", revision: 1,
    flags: { flag: { mode: "custom", action: "enable", groups: [[{ k: "age", op: "ge", v: 18 }]] } } };
  const server = http.createServer((request, response) => {
    requests++;
    if (requests > 1) return;
    response.writeHead(200, { ETag: '"one"' });
    response.end(JSON.stringify(policy));
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const client = Client.builder().url(`http://127.0.0.1:${server.address().port}`).environment("development").applicationId("app_test").secretKey("key").build();
  try {
    await Promise.all(Array.from({ length: 10 }, () => client.init()));
    assert.equal(requests, 1);
    assert.equal(client.evaluate("flag", { age: 21 }), Decision.ENABLED);
    assert.equal(client.evaluate("flag", { age: 17 }), Decision.DISABLED);
    assert.throws(() => client.evaluate("flag", { age: "private-value" }), error =>
      error instanceof ContextTypeError && error instanceof InvalidContextError &&
      error.k === "age" && error.op === "ge" && error.expected_type === "number" &&
      error.actual_type === "string" && !error.message.includes("private-value"));
    for (const age of [NaN, Infinity, 9007199254740992, null, {}, [], undefined, () => 18]) {
      assert.throws(() => client.evaluate("flag", { age }), InvalidContextError);
    }
    assert.throws(() => client.evaluate("flag", { age: 21, [Symbol("id")]: "private" }), InvalidContextError);
    const started = once(server, "request");
    const refresh = client.refresh();
    const rejected = assert.rejects(refresh, ClientClosedError);
    await started;
    assert.equal(client.evaluate("flag", { age: 21 }), Decision.ENABLED);
    await client.close();
    await rejected;
    assert.equal(client.evaluate("flag", { age: 21 }), Decision.ENABLED);
    await client.close();
  } finally {
    await client.close();
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  }
});

test("context capture preserves values and shared error details", async () => {
  const policy = { schema_version: 1, application_id: "app_test", project: "default", environment: "development", revision: 1,
    flags: {
      adult: { mode: "custom", action: "enable", groups: [[{ k: "age", op: "ge", v: 18 }]] },
      huge: { mode: "custom", action: "enable", groups: [[{ k: "age", op: "eq", v: 9007199254740992 }]] },
      always: { mode: "enabled" },
    } };
  const server = http.createServer((_request, response) => {
    response.writeHead(200, { ETag: '"one"' });
    response.end(JSON.stringify(policy));
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  const client = Client.builder().url(`http://127.0.0.1:${server.address().port}`).environment("development").applicationId("app_test").secretKey("key").build();
  try {
    await client.init();
    const context = { age: 17 };
    Object.defineProperty(context, "toJSON", { value: () => ({ age: 21 }) });
    assert.equal(client.evaluate("adult", context), Decision.DISABLED);
    let reads = 0;
    assert.equal(client.evaluate("huge", {
      get age() { return ++reads === 1 ? 18 : 9007199254740992; },
    }), Decision.DISABLED);
    assert.equal(reads, 1);
    for (const [age, actual_type] of [[null, "null"], [[], "array"], [{}, "object"]]) {
      assert.throws(() => client.evaluate("adult", { age }), error => {
        assert.ok(error instanceof ContextTypeError);
        assert.deepEqual(
          [error.flag_key, error.k, error.op, error.expected_type, error.actual_type],
          ["adult", "age", "ge", "number", actual_type],
        );
        return true;
      });
      assert.throws(() => client.evaluate("always", { age }), error =>
        error instanceof ContextTypeError && error.op === null &&
        error.expected_type === "string, number, or boolean" && error.actual_type === actual_type);
    }
    assert.throws(() => client.evaluate("adult", { extra: null }), error =>
      error instanceof InvalidContextError && !(error instanceof ContextTypeError) && error.k === "age");
    for (const invalid of [null, [], { age: null }, { age: NaN }, { age: 1n }]) {
      assert.throws(() => client.evaluate("missing", invalid), error =>
        error instanceof FlagNotFoundError && error.flag_key === "missing");
    }
    const nested = { toJSON() { throw new Error("Must not run"); } };
    assert.throws(() => client.evaluate("adult", { age: nested }), ContextTypeError);
    Object.defineProperty(Object.prototype, "toJSON", {
      configurable: true, value() { throw new Error("Must not run"); },
    });
    try {
      assert.equal(client.evaluate("adult", { age: 17 }), Decision.DISABLED);
      assert.throws(() => client.evaluate("adult", { age: [] }), ContextTypeError);
    } finally {
      delete Object.prototype.toJSON;
    }
  } finally {
    await client.close();
    server.closeAllConnections();
    await new Promise(resolve => server.close(resolve));
  }
});

test("application scope is required and validated", () => {
  for (const value of [undefined, "", "Bad", "app name", "app\n", "x".repeat(65)]) {
    assert.throws(() => Client.builder().url("http://localhost").secretKey("key").environment("development").applicationId(value).build(), TypeError);
    assert.throws(() => Client.builder().url("http://localhost").secretKey("key").environment(value).applicationId("app_test").build(), TypeError);
  }
});
