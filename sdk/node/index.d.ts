// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

export type Context = Record<string, string | number | boolean>;
export const Decision: Readonly<{ ENABLED: "enabled"; DISABLED: "disabled" }>;
export type Decision = typeof Decision[keyof typeof Decision];
export class PartlyError extends Error {}
export class FlagNotFoundError extends PartlyError { flag_key: string }
export class InvalidContextError extends PartlyError {}
export class ContextTypeError extends InvalidContextError {
  flag_key: string;
  k: string;
  op: string | null;
  expected_type: string;
  actual_type: string;
}
export class ResultTypeError extends PartlyError { flag_key: string; expected_type: string; actual_type: string }
export class NamedTypeError extends ResultTypeError {}
export interface Resolution { value: string | number | boolean | Context; kind: string; variant: string; named_type?: string }
export class PolicyNotReadyError extends PartlyError {}
export class AuthenticationError extends PartlyError {}
export class KeyRevokedError extends AuthenticationError {}
export class InvalidPolicyError extends PartlyError {}
export class RefreshError extends PartlyError {}
export class ClientClosedError extends PartlyError {}
export class ClientBuilder {
  url(value: string): this;
  secretKey(value: string): this;
  environment(value: string): this;
  applicationId(value: string): this;
  pollInterval(seconds: number): this;
  timeout(seconds: number): this;
  caBundle(path: string): this;
  build(): Client;
}
export class Client {
  hasFlag(flag: string): boolean;
  private constructor();
  static builder(): ClientBuilder;
  readonly lastRefreshError: PartlyError | null;
  init(): Promise<void>;
  refresh(): Promise<boolean>;
  evaluate(flag: string, context?: Context): Decision;
  resolve(flag: string, kind: string, context?: Context, expectedType?: string | null): Resolution;
  evaluateString(flag: string, context?: Context): string;
  evaluateInteger(flag: string, context?: Context): number;
  evaluateFloat(flag: string, context?: Context): number;
  evaluateObject(flag: string, context?: Context): Context;
  evaluateNamed(flag: string, expectedType: string, context?: Context): string | Context;
  close(): Promise<void>;
}
