# SDK contract tests

One end-to-end contract runs against Python, Rust, Go, and Node.js. Each language driver calls its real SDK. The Rust harness starts a temporary HTTPS server, changes flags, and checks the driver's results.

All four SDKs must pass before committing. Missing packages, build errors, and missing toolchains fail normally; nothing is skipped or marked as an expected failure.

## Run

Follow the [local setup guide](../../README.md) and install the [test dependencies](../../docs/releasing.md#build-and-verify). Rust, Go, and Node.js need their respective toolchains on `PATH`.

From the repository root, with the Python virtual environment active:

```sh
cargo build --locked -p partly-server
cargo test --locked -p partly-e2e --test e2e sdk::contract:: -- --test-threads=4
cargo test --locked -p partly-e2e --test e2e sdk::contract::python::
cargo test --locked -p partly-e2e --test e2e sdk::contract::rust::
cargo test --locked -p partly-e2e --test e2e sdk::contract::go::
cargo test --locked -p partly-e2e --test e2e sdk::contract::node::
```

The default server is `target/debug/partly`; `PARTLY_BINARY` can select an absolute path. See the [E2E guide](../e2e/README.md) for browser tests and failure artifacts.

Python uses the installed wheel. Rust builds against `sdk/rust`; Go uses a local module replacement for `sdk/go`; Node.js loads the package entry from `sdk/node`. The fixture builds the Go static library and Node native extension first. Go builds use `-a` because its build cache does not track changes to the Rust archive.

To verify release packages, set `PARTLY_SDK_PACKAGES` to the output of `scripts/package_sdks.py`. The same contracts then run against extracted crates, a standalone Go archive, and an npm-installed Node tarball in temporary application directories. Missing archives or native libraries fail normally.

## What it checks

- Construction without network access; initialization failure, retry, authentication, and certificate verification.
- Local on/off decisions, fixed percentage vectors, stable membership as the population grows, and increased rollout.
- Every rule operator, AND/OR groups, both match actions, and explicit errors for unknown flags, missing context, and wrong types.
- Exact large integers, floating-point comparisons, and structured type-error details across languages.
- Unchanged and changed refresh responses, environment isolation, and background polling.
- Ordinary outages preserve accepted policies; cold starts fail. Confirmed key revocation clears the policy and stops evaluation.
- Invalid or oversized policy responses, invalid ETags, changed scope, redirects, and whole-download timeouts. Rejected responses preserve both the last policy and its ETag.
- Repeated shutdown, local evaluation after closing, and rejected network operations after closing.

The existing SDK unit tests continue to cover detailed validation and concurrency cases.

## OpenFeature contract

`cargo test --locked -p partly-e2e --test e2e openfeature` exercises the official OpenFeature SDKs against the real server, including changed/unchanged refresh, local evaluation during an outage, and confirmed revocation. All four languages use JSON-line drivers. Rust returns errors through `Result`; the other official SDKs return caller defaults with error details.

The Go OpenFeature driver has its own Go 1.26+ module so it does not raise the native SDK's Go 1.22 minimum. Provider unit tests also cover typed getters, context precedence, hooks, lifecycle, and isolation. With `PARTLY_SDK_PACKAGES` set, the same tests use extracted release crates and Go modules, an npm-installed tarball, and an installed Python wheel. An editable Python install fails this mode. Release CI also runs the dashboard quickstarts against these packages. See the [OpenFeature evidence and limits](../../docs/openfeature.md#coverage-and-limits).

## Driver contract

Drivers read one JSON command per line from stdin and write one JSON result per line to stdout. They hold a real client between commands. Credentials stay out of command arguments; diagnostics go to stderr.

| Command | Inputs | Successful result |
| --- | --- | --- |
| `build` | `url`, `key`, `environment`, `application_id`, `interval`, `timeout`, optional `ca_bundle`; times in seconds | `{"ok": true}` |
| `init` | None | `{"ok": true}` |
| `evaluate` | `flag`, `context` | `{"decision": "enabled"}` or `{"decision": "disabled"}` |
| `refresh` | None | `{"updated": true}` or `{"updated": false}` |
| `close` | None | `{"ok": true}` |

SDK errors return `{"error": "code"}`. Codes are `flag_not_found`, `invalid_context`, `context_type`, `policy_not_ready`, `authentication`, `key_revoked`, `invalid_policy`, `refresh`, and `client_closed`. Unexpected errors terminate the driver and fail the test.

`context_type` also returns `flag_key`, `k`, `op`, `expected_type`, and `actual_type`; `op` is `null` when no rule operator applies. Drivers preserve numeric precision when reading commands.

Drivers use the implemented public client APIs. Preserve the shared assertions when changing those APIs. Drivers must not evaluate policies, make policy HTTP requests themselves, or invent successful results.
