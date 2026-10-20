// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

"use strict";
const http = require("node:http");
const https = require("node:https");
const tls = require("node:tls");
const { readFile } = require("node:fs/promises");
const { once } = require("node:events");
const native = require(`./native/${process.platform}-${process.arch}/partly.node`);

class PartlyError extends Error {
  constructor(message) { super(message); this.name = new.target.name; }
}
class FlagNotFoundError extends PartlyError {}
class InvalidContextError extends PartlyError {}
class ContextTypeError extends InvalidContextError {}
class PolicyNotReadyError extends PartlyError {}
class AuthenticationError extends PartlyError {}
class InvalidPolicyError extends PartlyError {}
class RefreshError extends PartlyError {}
class ClientClosedError extends PartlyError {}

const Decision = Object.freeze({ ENABLED: "enabled", DISABLED: "disabled" });
const evaluationErrors = {
  flag_not_found: FlagNotFoundError, invalid_context: InvalidContextError, context_type: ContextTypeError,
};

class ClientBuilder {
  #options = { interval: 30, timeout: 5 };
  url(value) { this.#options.url = value; return this; }
  secretKey(value) { this.#options.key = value; return this; }
  pollInterval(value) { this.#options.interval = value; return this; }
  timeout(value) { this.#options.timeout = value; return this; }
  caBundle(value) { this.#options.ca = value; return this; }
  build() {
    const options = { ...this.#options };
    let url;
    try { url = new URL(options.url); } catch { throw new TypeError("Supply an HTTP(S) control-plane URL"); }
    // eslint-disable-next-line no-control-regex -- Reject URL control characters before the parser normalizes them.
    if (typeof options.url !== "string" || /[\s\x00-\x20\x7f\\@]/u.test(options.url) ||
        !["http:", "https:"].includes(url.protocol) || !url.hostname ||
        url.username || url.password || url.pathname !== "/" || /[?#]/u.test(options.url)) {
      throw new TypeError("Supply an HTTP(S) origin without credentials or a path");
    }
    if (typeof options.key !== "string" || !/^[\x21-\x7e]+$/u.test(options.key)) {
      throw new TypeError("Supply a nonempty ASCII API Key without whitespace");
    }
    for (const value of [options.interval, options.timeout]) {
      if (typeof value !== "number" || !Number.isFinite(value) || value <= 0 || value * 1000 > 2147483647) {
        throw new TypeError("Interval and timeout must be positive seconds, at most 2147483");
      }
    }
    if (options.ca !== undefined && (typeof options.ca !== "string" || !options.ca || options.ca.includes("\0"))) {
      throw new TypeError("Supply a nonempty CA bundle path");
    }
    options.url = new URL("/api/policy", url);
    return new Client(options);
  }
}

class Client {
  #options;
  #agent;
  #snapshot;
  #etag;
  #closed = false;
  #queue = Promise.resolve();
  #timer;
  #abort = new AbortController();
  #lastError = null;
  constructor(options) { this.#options = options; }
  static builder() { return new ClientBuilder(); }
  get lastRefreshError() { return this.#lastError; }
  #checkOpen() { if (this.#closed) throw new ClientClosedError("Client is closed"); }
  #serial(operation) {
    const next = this.#queue.then(operation);
    this.#queue = next.catch(() => {});
    return next;
  }
  init() {
    return this.#serial(async () => {
      this.#checkOpen();
      if (this.#snapshot) return;
      if (!this.#agent) {
        try {
          const ca = this.#options.ca ? await readFile(this.#options.ca) : tls.rootCertificates;
          this.#checkOpen();
          this.#agent = this.#options.url.protocol === "https:"
            ? new https.Agent({ keepAlive: true, ca, rejectUnauthorized: true, minVersion: "TLSv1.2" })
            : new http.Agent({ keepAlive: true });
        } catch (error) {
          if (error instanceof ClientClosedError) throw error;
          throw new RefreshError("Cannot load TLS trust certificates");
        }
      }
      await this.#download();
      this.#checkOpen();
      this.#schedule();
    });
  }
  refresh() {
    return this.#serial(async () => {
      this.#checkOpen();
      if (!this.#snapshot) throw new PolicyNotReadyError("Await init first");
      return this.#download();
    });
  }
  #schedule() {
    if (this.#closed) return;
    this.#timer = setTimeout(async () => {
      try { await this.refresh(); } catch (error) { this.#lastError = error; }
      this.#schedule();
    }, this.#options.interval * 1000);
    this.#timer.unref();
  }
  async #request() {
    const headers = { Authorization: `Bearer ${this.#options.key}` };
    if (this.#etag) headers["If-None-Match"] = this.#etag;
    const transport = this.#options.url.protocol === "https:" ? https : http;
    const request = transport.get(this.#options.url, {
      headers, agent: this.#agent, signal: this.#abort.signal, rejectUnauthorized: true,
    });
    // Keep an error listener after response headers arrive; stream errors are handled below.
    request.on("error", () => {});
    const timer = setTimeout(() => request.destroy(new RefreshError("Policy request timed out")), this.#options.timeout * 1000);
    try {
      const [response] = await once(request, "response");
      const status = response.statusCode;
      const tag = response.headers.etag;
      if (status !== 200) {
        response.destroy();
        return { status, tag };
      }
      const chunks = [];
      let length = 0;
      for await (const chunk of response) {
        length += chunk.length;
        if (length > 1024 * 1024) throw new InvalidPolicyError("Policy exceeds 1 MiB");
        chunks.push(chunk);
      }
      return { status, tag, body: Buffer.concat(chunks, length) };
    } finally {
      clearTimeout(timer);
      request.destroy();
    }
  }
  async #download() {
    try {
      const { status, tag, body } = await this.#request();
      this.#checkOpen();
      if (status === 401 || status === 403) throw new AuthenticationError("API Key rejected");
      if (status === 304) {
        if (!this.#snapshot || (tag !== undefined && tag !== this.#etag)) {
          throw new InvalidPolicyError("Unchanged response has no matching policy");
        }
        this.#lastError = null;
        return false;
      }
      if (status !== 200) throw new RefreshError(`Policy request returned HTTP ${status}`);
      if (typeof tag !== "string" || !/^"[\x21\x23-\x7e]+"$/u.test(tag)) {
        throw new InvalidPolicyError("Missing or invalid ETag");
      }
      let snapshot;
      try { snapshot = await native.parsePolicy(body); }
      catch { throw new InvalidPolicyError("Policy validation failed"); }
      this.#checkOpen();
      if (this.#snapshot && this.#snapshot.scope !== snapshot.scope) throw new InvalidPolicyError("Policy scope changed");
      this.#snapshot = snapshot;
      this.#etag = tag;
      this.#lastError = null;
      return true;
    } catch (error) {
      const failure = this.#closed ? new ClientClosedError("Client is closed")
        : error instanceof PartlyError ? error : new RefreshError("Network, timeout, or TLS failure");
      this.#lastError = failure;
      throw failure;
    }
  }
  evaluate(flag, context = {}) {
    if (!this.#snapshot) throw new PolicyNotReadyError("Await init first");
    if (typeof flag !== "string") throw new TypeError("Flag key must be a string");
    if (!this.#snapshot.containsFlag(flag)) {
      throw Object.assign(new FlagNotFoundError(`Flag '${flag}' is not defined in the loaded policy`), { flag_key: flag });
    }
    if (context === null || typeof context !== "object" || Object.getOwnPropertySymbols(context).length ||
        ![Object.prototype, null].includes(Object.getPrototypeOf(context))) {
      throw new InvalidContextError("Context must be a flat object");
    }
    const values = Object.create(null);
    for (const [k, value] of Object.entries(context)) {
      if (typeof value === "number" && (!Number.isFinite(value) || (Number.isInteger(value) && !Number.isSafeInteger(value)))) {
        throw new InvalidContextError("Numbers must be finite; integers must be safe JavaScript integers");
      }
      // Capture primitives once; inert placeholders let Rust report the selected rule's expected type.
      values[k] = value === null || ["string", "number", "boolean"].includes(typeof value)
        ? value : Array.isArray(value) ? Object.setPrototypeOf([], null) : Object.create(null);
    }
    const request = Object.assign(Object.create(null), { flag, context: values });
    const result = JSON.parse(this.#snapshot.evaluate(JSON.stringify(request)));
    if (result.error) {
      const ErrorType = evaluationErrors[result.error];
      if (!ErrorType) throw new PartlyError("Unknown native evaluation error");
      const { error: _, message, ...details } = result;
      throw Object.assign(new ErrorType(message), details);
    }
    if (result.decision !== Decision.ENABLED && result.decision !== Decision.DISABLED) throw new PartlyError("Unknown native decision");
    return result.decision;
  }
  async close() {
    this.#closed = true;
    clearTimeout(this.#timer);
    this.#abort.abort();
    await this.#queue;
    this.#agent?.destroy();
  }
}

module.exports = {
  Client, ClientBuilder, Decision, PartlyError, FlagNotFoundError, InvalidContextError, ContextTypeError,
  PolicyNotReadyError, AuthenticationError, InvalidPolicyError, RefreshError, ClientClosedError,
};
