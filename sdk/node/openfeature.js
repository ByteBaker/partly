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
    if (error instanceof partly.ResultTypeError) throw new of.TypeMismatchError('Requested result type does not match the flag');
    if (error instanceof partly.InvalidContextError || error instanceof TypeError) throw new of.InvalidContextError('Context does not satisfy the flag requirements');
    throw new of.GeneralError('Partly evaluation failed');
  }
  #check(flag) {
    if (!this.#ready) throw new of.ProviderNotReadyError('Partly provider is not initialized');
    let exists;
    try { exists = this.#client.hasFlag(flag); } catch (error) { this.#raise(error); }
    if (!exists) throw new of.FlagNotFoundError('Flag is not defined');
  }
  #resolve(flag, kind, context = {}) {
    this.#check(flag);
    const attributes = Object.create(null);
    for (const [key, value] of Object.entries(context)) {
      if (['string', 'number', 'boolean'].includes(typeof value)) attributes[key] = value;
    }
    if (context.targetingKey !== undefined) attributes.id = context.targetingKey;
    let result;
    try { result = this.#client.resolve(flag, kind, attributes); }
    catch (error) { this.#raise(error); }
    return { value: result.value, variant: result.variant, reason: of.StandardResolutionReasons.CACHED,
      flagMetadata: result.named_type ? { 'partly.result_type': result.named_type } : {} };
  }
  async resolveBooleanEvaluation(flag, _defaultValue, context) { return this.#resolve(flag, 'boolean', context); }
  async resolveStringEvaluation(flag, _defaultValue, context) { return this.#resolve(flag, 'string', context); }
  async resolveNumberEvaluation(flag, _defaultValue, context) { return this.#resolve(flag, 'number', context); }
  async resolveObjectEvaluation(flag, _defaultValue, context) { return this.#resolve(flag, 'object', context); }
}

module.exports = { PartlyProvider };
