# SDK contract tests

One end-to-end contract runs against Python, Rust, Go, and Node.js, in that order. Each language driver calls its real SDK. Pytest starts a temporary HTTPS server, changes flags, and checks the driver's results.

All four SDKs must pass before committing. Missing packages, build errors, and missing toolchains fail normally; nothing is skipped or marked as an expected failure.

## Run

Follow the [local setup guide](../../README.md) and install the [test dependencies](../../docs/releasing.md#build-and-verify). Rust, Go, and Node.js need their respective toolchains on `PATH`.

From the repository root, with the Python virtual environment active:

```sh
cargo build --locked -p partly-sdk-contract
python -m pytest -q tests/e2e/test_sdk_contract.py
python -m pytest -q tests/e2e/test_sdk_contract.py -k python
python -m pytest -q tests/e2e/test_sdk_contract.py -k rust
python -m pytest -q tests/e2e/test_sdk_contract.py -k go
python -m pytest -q tests/e2e/test_sdk_contract.py -k node
```

The default server is `target/debug/partly`; `PARTLY_BINARY` can select an absolute path. The normal `python -m pytest` run also includes this contract.

Python uses the installed wheel. Rust builds against `sdk/rust`; Go uses a local module replacement for `sdk/go`; Node.js loads the package entry from `sdk/node`. The fixture builds the Go static library and Node native extension first. Go builds use `-a` because its build cache does not track changes to the Rust archive.

To verify release packages, set `PARTLY_SDK_PACKAGES` to the output of `scripts/package_sdks.py`. The same contracts then run against extracted crates, a standalone Go archive, and an npm-installed Node tarball in temporary application directories. Missing archives or native libraries fail normally.

## What it checks

- Construction without network access; initialization failure, retry, authentication, and certificate verification.
- Local on/off decisions, fixed percentage vectors, stable membership as the population grows, and increased rollout.
- Every rule operator, AND/OR groups, both match actions, and explicit errors for unknown flags, missing context, and wrong types.
- Exact large integers, floating-point comparisons, and structured type-error details across languages.
- Unchanged and changed refresh responses, environment isolation, and background polling.
- Key revocation and server outages preserve accepted policies; cold starts fail.
- Invalid or oversized policy responses, invalid ETags, changed scope, redirects, and whole-download timeouts. Rejected responses preserve both the last policy and its ETag.
- Repeated shutdown, local evaluation after closing, and rejected network operations after closing.

The existing SDK unit tests continue to cover detailed validation and concurrency cases.

## Driver contract

Drivers read one JSON command per line from stdin and write one JSON result per line to stdout. They hold a real client between commands. Credentials stay out of command arguments; diagnostics go to stderr.

| Command | Inputs | Successful result |
| --- | --- | --- |
| `build` | `url`, `key`, `interval`, `timeout`, optional `ca_bundle`; times in seconds | `{"ok": true}` |
| `init` | None | `{"ok": true}` |
| `evaluate` | `flag`, `context` | `{"decision": "enabled"}` or `{"decision": "disabled"}` |
| `refresh` | None | `{"updated": true}` or `{"updated": false}` |
| `close` | None | `{"ok": true}` |

SDK errors return `{"error": "code"}`. Codes are `flag_not_found`, `invalid_context`, `context_type`, `policy_not_ready`, `authentication`, `invalid_policy`, `refresh`, and `client_closed`. Unexpected errors terminate the driver and fail the test.

`context_type` also returns `flag_key`, `k`, `op`, `expected_type`, and `actual_type`; `op` is `null` when no rule operator applies. Drivers preserve numeric precision when reading commands.

Drivers use the implemented public client APIs. Preserve the shared assertions when changing those APIs. Drivers must not evaluate policies, make policy HTTP requests themselves, or invent successful results.
