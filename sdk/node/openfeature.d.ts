// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

import type { EvaluationContext, Provider, ResolutionDetails } from '@openfeature/server-sdk';
import type { Client } from './index';

export class PartlyProvider implements Provider {
  readonly metadata: { readonly name: 'Partly' };
  readonly runsOn: 'server';
  constructor(client: Client);
  initialize(): Promise<void>;
  onClose(): Promise<void>;
  resolveBooleanEvaluation(flag: string, defaultValue: boolean, context?: EvaluationContext): Promise<ResolutionDetails<boolean>>;
  resolveStringEvaluation(flag: string): Promise<never>;
  resolveNumberEvaluation(flag: string): Promise<never>;
  resolveObjectEvaluation(flag: string): Promise<never>;
}
