// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

"use strict";
const of = require('@openfeature/server-sdk');
const partly = require('./index');

// The application owns the Partly client's asynchronous initialization and cleanup.
class PartlyProvider {
  metadata = Object.freeze({ name: 'Partly' });
  runsOn = 'server';
  #client;
  #ready = false;
  constructor(client) { this.#client = client; }
  async initialize() {
    this.#ready = false;
    try { this.#client.hasFlag(''); } catch (error) { this.#raise(error); }
    this.#ready = true;
  }
  async onClose() { this.#ready = false; }
  #raise(error) {
    if (error instanceof partly.KeyRevokedError) throw new of.ProviderFatalError('API Key revoked');
    if (error instanceof partly.PolicyNotReadyError) throw new of.ProviderNotReadyError('Await Partly client.init() before registering the provider');
    if (error instanceof partly.FlagNotFoundError) throw new of.FlagNotFoundError('Flag is not defined');
    if (error instanceof partly.InvalidContextError || error instanceof TypeError) throw new of.InvalidContextError('Context does not satisfy the flag requirements');
    throw new of.GeneralError('Partly evaluation failed');
  }
  #check(flag) {
    if (!this.#ready) throw new of.ProviderNotReadyError('Partly provider is not initialized');
    let exists;
    try { exists = this.#client.hasFlag(flag); } catch (error) { this.#raise(error); }
    if (!exists) throw new of.FlagNotFoundError('Flag is not defined');
  }
  async resolveBooleanEvaluation(flag, _defaultValue, context = {}) {
    this.#check(flag);
    const attributes = Object.create(null);
    for (const [key, value] of Object.entries(context)) {
      if (['string', 'number', 'boolean'].includes(typeof value)) attributes[key] = value;
    }
    if (context.targetingKey !== undefined) attributes.id = context.targetingKey;
    let value;
    try { value = this.#client.evaluate(flag, attributes) === partly.Decision.ENABLED; }
    catch (error) { this.#raise(error); }
    return { value, variant: value ? 'enabled' : 'disabled', reason: of.StandardResolutionReasons.CACHED };
  }
  #unsupported(flag) { this.#check(flag); throw new of.TypeMismatchError('Partly flags are Boolean'); }
  async resolveStringEvaluation(flag) { return this.#unsupported(flag); }
  async resolveNumberEvaluation(flag) { return this.#unsupported(flag); }
  async resolveObjectEvaluation(flag) { return this.#unsupported(flag); }
}

module.exports = { PartlyProvider };
