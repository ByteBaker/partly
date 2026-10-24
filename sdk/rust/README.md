# Partly Rust SDK

Async policy downloads through Reqwest and rustls; local decisions through `partly-core`. Initialize one client at application startup, before accepting requests. Store it in application state, pass it to request handlers, and close it at shutdown. Use `Arc<Client>` to share ownership between tasks.

## Use a release archive

Requires Rust 1.89+. Until publication, extract the `partly-core` and `partly-rust` `.crate` archives into `third_party`, then use:

```toml
[dependencies]
partly = { package = "partly-rust", version = "=0.1.0-alpha.1", path = "third_party/partly-rust-0.1.0-alpha.1", features = ["openfeature"] }
open-feature = "0.3.0"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }

[patch.crates-io]
partly-core = { path = "third_party/partly-core-0.1.0-alpha.1" }
```

The archives build without this checkout. For development inside the repository, use `sdk/rust` as the dependency path.

Use the [complete OpenFeature quickstart](https://github.com/ByteBaker/partly/blob/main/examples/rust/quickstart.rs) for connection settings, provider registration, two requests sharing one initialized client, error handling, and shutdown. Handlers only evaluate locally. See [context mapping and upstream Rust limitations](https://github.com/ByteBaker/partly/blob/main/docs/openfeature.md#rust).

## Native client reference

- `build()` validates settings without network access. `init().await` downloads the first policy and starts one poller; failed initialization can be retried.
- Malformed URLs return `Error::UrlParse(UrlParseError)` with the original parser error, also available through `std::error::Error::source()`. Valid URLs that violate the allowed origin format return `Error::Configuration`. Neither error includes the supplied URL or API Key.
- `evaluate()` returns `Result<Decision, Error>`. Match `Error::Evaluation(EvaluationError::ContextType { .. })` for wrong context types. Unknown flags and missing policy are separate errors.
- `refresh().await` returns `true` for an accepted policy, `false` for an unchanged response. Ordinary failures preserve the policy and ETag; confirmed revocation clears them.
- `close().await` cancels downloads and stops polling. Repeated calls are safe. The last policy remains available for evaluation, but the client cannot reconnect.
- `last_refresh_error()` reports a background refresh failure; a successful refresh clears it.

Builder `poll_interval(Duration)` defaults to 30 seconds and `timeout(Duration)` to 5 seconds for the complete HTTP download. Policy validation runs outside the async executor.

HTTPS verifies certificates and hostnames using bundled public roots. `ca_bundle(path)` replaces them with private PEM roots, loaded during initialization. Redirects and environment proxies are disabled. Errors omit request credentials and response bodies.

Context is a flat map of strings, numbers, and booleans. See [shared evaluation rules](https://github.com/ByteBaker/partly/blob/main/crates/partly-core/README.md) for types, operators, and stable percentage selection.

```sh
cargo test -p partly-rust
cargo test --locked -p partly-e2e --test e2e sdk::contract::rust::
```

[Apache-2.0](LICENSE).

## Application scope and revocation

Create an application in **Apps & Keys** and select the flags it can use. Supply its environment and permanent application ID on the builder with `.environment("development").application_id("YOUR_APPLICATION_ID")`, along with its API Key. All are required; the key must match both scope values. Even the first download is checked against this scope. Removing a selected flag makes it unknown after refresh.

A confirmed revoked-key response clears policy and ETag, stops polling, and makes evaluation, initialization, and refresh return `Error::KeyRevoked`. Build a new client with a valid replacement key. Closing a revoked client does not restore evaluation. Ordinary authentication, network, and validation failures preserve the accepted policy. Offline clients learn about revocation only when a refresh or usage report reaches the server.

## OpenFeature

Enable the `openfeature` Cargo feature to use `partly::openfeature::PartlyProvider` with the official `open-feature` crate. Initialize an `Arc<Client>` once before registering the provider. Registration makes no network requests; evaluations use the same local policy.

The upstream Rust SDK returns `Result` instead of accepting caller defaults. It also lacks provider events and a dedicated fatal-error code, so full specification conformance is not claimed. Keep the native client for refresh and close it at shutdown. See the [integration guide](https://github.com/ByteBaker/partly/blob/main/docs/openfeature.md#rust) and [tested compatibility](https://github.com/ByteBaker/partly/blob/main/docs/openfeature.md#coverage-and-limits).

## Flag usage reporting

Evaluations collect the distinct known flag names used by this application, including disabled decisions and invalid-context errors. Unknown flags and existence checks are not counted. Reports contain flag names only, never context values, decisions, or evaluation counts.

A later evaluation sends up to 128 names in the background when 128 distinct names are waiting or 60 seconds have passed since initialization or the last report. There is no reporting timer. Only one report runs at a time; failures retain current flag names and wait at least 60 seconds before another evaluation can retry. Policy refresh removes names that are no longer assigned to the application.

Reporting never waits for a network response during evaluation. Ordinary reporting failures do not change decisions, policy, or refresh diagnostics. Confirmed key revocation uses the same terminal behavior as policy refresh. Closing cancels reporting and discards pending names without sending a final report; cached evaluation after close does not collect usage. These reports are best effort: idle clients, shutdown, or crashes can leave recent usage unreported.
