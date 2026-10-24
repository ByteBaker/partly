# End-to-end tests

One Rust harness starts real Partly processes in temporary directories. It tests
the HTTP API, SQLite persistence and recovery, TLS, SDKs, examples, and dashboard.
Your development database is never used.

## Layout

```text
tests/e2e/
├── Cargo.toml
├── README.md
├── src/bin/install_browsers.rs
└── tests/
    ├── e2e.rs          # One integration-test target
    ├── api/            # HTTP API, permissions, storage, recovery, and TLS
    ├── sdk/            # Shared language contracts, examples, and Go relinking
    ├── dashboard/      # Browser workflows, keyboard access, and layout
    └── support/        # Server fixtures, language drivers, HTTP stubs, browser helpers
```

Cargo discovers `tests/e2e.rs` as the `e2e` target. It includes the suites as
modules; shared helpers are compiled once within that target.

## Run

Install the development tools and Python 3.14 SDK as described in
[release checks](../../docs/releasing.md). Activate that Python environment first.
Rust, Go, and Node must also be available; missing SDKs or tools fail the tests.

```sh
cargo build --locked -p partly-server

# API, operations, native SDKs, OpenFeature, and examples; no browsers needed.
cargo test --locked -p partly-e2e --test e2e -- --test-threads=4

# Install browsers matching the pinned playwright-rs driver.
# Add --with-deps on Linux to install browser system dependencies.
cargo run --locked -p partly-e2e --features browser --bin partly-test-browsers -- chromium firefox webkit

cargo test --locked -p partly-e2e --features browser --test e2e dashboard:: -- --test-threads=4
PARTLY_BROWSER=firefox cargo test --locked -p partly-e2e --features browser --test e2e dashboard:: -- --test-threads=4
PARTLY_BROWSER=webkit cargo test --locked -p partly-e2e --features browser --test e2e dashboard:: -- --test-threads=4
```

Use a module filter to select `api::`, `sdk::`, or `dashboard::`, or narrow it to
`sdk::contract::python::` or `api::operations::`.
`PARTLY_BINARY` selects a built server; `PARTLY_PYTHON` selects
the Python interpreter. Both accept absolute paths. `PARTLY_SDK_PACKAGES` selects
release archives to test from fresh consumer directories; install the matching
Python wheel first.

Browser failures save a screenshot and Playwright trace under
`test-results/<browser>/<test>/`. These files are ignored by Git. Each browser
test closes its context on success, failure, or timeout.

## Coverage

| Files | Checks |
| --- | --- |
| `tests/api/{control_plane,applications,rbac}.rs` | Setup, authentication, scoped flags and keys, confirmation, audits, roles |
| `tests/api/usage.rs` | Exact per-key/flag writes, scope, rollback, retention, revocation, and schema compatibility |
| `tests/api/{operations,https}.rs` | Restart, crash recovery, backups, password recovery, TLS validation, graceful shutdown |
| `tests/sdk/contract.rs` | Four native SDKs and OpenFeature providers, typed rules, refresh, offline cache, revocation, invalid responses |
| `tests/sdk/usage.rs` | Real native and OpenFeature usage batches in all four languages, HTTPS, deduplication, and shutdown |
| `tests/sdk/{examples,go_build}.rs` | Runnable documentation, package installation, native Go relinking |
| `tests/dashboard/` | Forms, navigation, keyboard access, responsive layout, feedback, audits, users, SDK integration |

Language drivers in `../sdk_contract/` call the actual SDK bindings. Python remains
necessary to test the Python SDK; it no longer runs the E2E harness. SDK unit tests
remain in their own languages.

Routine CI runs compiler and lint checks with `PLAYWRIGHT_SKIP_DRIVER_DOWNLOAD=1`.
Build checks run Chromium; release checks also run Firefox, WebKit, and isolated
package consumers. Workspace unit tests exclude `partly-e2e` so E2E runs once in
its own step.
