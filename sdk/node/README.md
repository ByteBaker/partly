# Partly Node.js SDK

Async HTTP downloads with local Rust evaluation through Node-API. Supports server-side Node.js 22+; not browsers.

## Build locally

Requires Rust and a C compiler:

```sh
node sdk/node/build.js
node examples/node/quickstart.js
```

Replace the constants in [the example](../../examples/node/quickstart.js) first. It loads the local SDK. The package is private until its public name and distribution are confirmed.

The build creates `sdk/node/partly.node` for the current platform. No npm dependencies are needed. Prebuilt public packages are not available yet.

## Client contract

Build with `Client.builder().url(url).secretKey(key).build()`. Reuse one instance; building it makes no requests.

| Method | Behavior |
| --- | --- |
| `await init()` | Downloads the first policy, then starts one background poller |
| `evaluate(flag, context)` | Returns `Decision.ENABLED` or `Decision.DISABLED` locally |
| `await refresh()` | Returns `true` for an accepted update, `false` for an unchanged response |
| `await close()` | Cancels downloads and stops polling; safe to repeat |

Compare decisions explicitly with `=== Decision.ENABLED`; both decision strings are truthy. Errors extend `PartlyError`: `FlagNotFoundError`, `PolicyNotReadyError`, `InvalidContextError`, `ContextTypeError`, `AuthenticationError`, `InvalidPolicyError`, `RefreshError`, and `ClientClosedError`. Wrong context types expose `flag_key`, `k`, `op`, `expected_type`, and `actual_type`.

Defaults: 30-second polling, 5-second whole-download timeout, 1 MiB policies. Builder `pollInterval(seconds)` and `timeout(seconds)` accept positive seconds up to 2,147,483. `lastRefreshError` reports refresh failures; successful refresh clears it.

HTTPS verifies certificates and hostnames using Node's bundled public roots. `caBundle(path)` replaces those roots with a private PEM bundle. Redirects and environment proxies are disabled. Policy parsing runs on a native worker, outside the event loop.

Context must be a flat object of strings, finite numbers, and booleans. Integer context values must be safe JavaScript integers (±9,007,199,254,740,991). BigInt, null, nested objects, and arrays are rejected. Policy numbers retain their original precision in Rust. See [shared rules](../../crates/partly-core/README.md).

Failed refreshes keep the policy and ETag. Local evaluation still works after close, but network calls then fail. Initialization can be retried after a failure.

```sh
node --test sdk/node/test/*.test.js
python -m pytest -q tests/e2e/test_sdk_contract.py -k node
```

[Apache-2.0](LICENSE). Local builds support macOS and Linux; only macOS ARM64 has been verified locally.
