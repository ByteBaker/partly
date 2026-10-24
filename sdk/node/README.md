# Partly Node.js SDK

Async HTTP downloads with local Rust evaluation through Node-API. Supports server-side Node.js 22+; not browsers. Await `init()` once at application startup, before accepting requests. Register the OpenFeature provider and keep both clients in application state for local flag checks; close it at application shutdown.

## Install a release archive

Install the `.tgz` for your platform with `npm install ./partly-node-<version>-<platform>.tgz @openfeature/server-sdk@^1.23.0`, then import from `partly-sdk`. It includes the native library; consumers do not need Rust or this checkout. Supported targets are macOS ARM64 and Linux x86_64. The tarballs are platform-specific; npm publishing remains separate.

The Linux archive requires glibc 2.34 or newer. It does not run on Alpine.

## Build from source

Requires Rust and a C compiler:

```sh
npm ci --ignore-scripts
node sdk/node/build.js
node examples/node/quickstart.js
```

Replace the constants in [the example](../../examples/node/quickstart.js) first. It imports `partly-sdk`; `npm ci` links the local workspace package. The package remains private until registry publication.

The build creates `sdk/node/native/<platform>-<arch>/partly.node`. Native SDK use has no npm runtime dependencies; the optional OpenFeature adapter uses the official server SDK.

## Native client reference

Build with `Client.builder().url(url).environment(env).applicationId(appID).secretKey(key).build()`. Reuse one instance; building it makes no requests.

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

Ordinary failed refreshes keep the policy and ETag. Local evaluation still works after close, but network calls then fail. Initialization can be retried after a failure.

## Flag usage reporting

Native evaluation records distinct known flag names, including disabled decisions and context errors. An evaluation starts a background report when 128 names are pending or 60 seconds have passed since initialization or the previous report. Reports contain only flag names; context values and decisions stay local. `hasFlag()`, unknown flags, and OpenFeature checks that fail before native evaluation do not count.

One report runs at a time, with up to 128 names. Failed reports retry only after another evaluation and a 60-second cooldown; they do not change decisions or `lastRefreshError`. Accepted policies remove pending names that are no longer assigned. Confirmed key revocation stops reporting and clears the policy. Closing cancels reporting and discards pending names; cached evaluation remains available without recording more usage. Usage is best effort, so short-lived or idle clients may leave names unreported.

## Tests

```sh
node --test sdk/node/test/*.test.js
cargo test --locked -p partly-e2e --test e2e sdk::contract::node::
```

[Apache-2.0](LICENSE).

## Application scope and revocation

Create an application in **Apps & Keys** and select the flags it can use. Supply its environment and permanent application ID on the builder with `.environment("development").applicationId("YOUR_APPLICATION_ID")`, along with its API Key. All are required; the key must match both scope values. Even the first download is checked against this scope. Removing a selected flag makes it unknown after refresh.

A confirmed revoked-key response clears policy and ETag, stops polling, and makes evaluation, initialization, and refresh return `KeyRevokedError` (an `AuthenticationError` subclass). Build a new client with a valid replacement key. Closing a revoked client does not restore evaluation. Ordinary authentication, network, and validation failures preserve the accepted policy. Offline clients learn about revocation only when a refresh or usage report reaches the server.

## OpenFeature

Install the optional peer `@openfeature/server-sdk` and import `PartlyProvider` from `partly-sdk/openfeature`. Initialize the native client once before registering the provider. Registration makes no network requests; evaluations use the same local policy.

OpenFeature returns your fallback on errors. Use detailed getters to inspect `errorCode` before accepting that value. Keep the native client for refresh and close it at shutdown. See the [integration guide](../../docs/openfeature.md#nodejs--typescript) and [tested compatibility](../../docs/openfeature.md#coverage-and-limits).
