# Partly

Partly is a small feature flag control plane. Run one Rust executable, set up an admin account in your browser, and connect a Python application. Flag checks run locally inside the SDK, even when the control plane is offline.

This is a local POC. It supports boolean decisions, stable percentage rollouts, local context rules, development and production environments, and named SDK keys. The dashboard and SQLite are included in the executable. Docker, Node.js, and an external database are not needed to run it.

## Run the control plane

Building from source needs a recent stable Rust toolchain and a C compiler. From this directory:

```sh
cargo build --release --locked -p partly-server
./target/release/partly serve --host 127.0.0.1 --port 8080 --db ./partly.db
```

Open <http://127.0.0.1:8080>. Create an admin account with a password of at least 12 characters. Partly creates the database only after valid setup. An existing invalid database causes a startup error; it is never replaced automatically.

Choose **Development** in the top-right environment selector. In **Feature flags**, select **Create flag**, enter `new_checkout`, and choose a rollout mode: enabled for everyone, percentage of users, custom rules, or disabled for everyone. Creation always uses the current environment. The environment is also in the URL; switching it reloads the page, with a warning before discarding unsaved edits.

Open **Key Management**, name and generate an SDK key, then select **Copy**. The key stays masked; **Reveal** remains available whenever you need to see it. **Docs** contains the complete Python quickstart, with a language dropdown and error handling.

The development database stays at schema v1 without upgrade migrations. Databases from the earlier on/off and percentage-only design are incompatible. Use a fresh path, such as `--db ./partly-rules.db`, to try this version; existing files are left untouched.

After building, the executable can be moved to another directory and run by itself on the same platform. The Python package is separate.

## Build and try the Python SDK

The SDK targets regular CPython 3.10+ using a native ABI3 extension. The full local checks use Python 3.12 on macOS ARM64; the focused SDK suite also passes on Python 3.14. Other platforms need their own wheel builds; free-threaded Python is not supported by this wheel.

```sh
python3 -m venv .venv
source .venv/bin/activate
python -m pip install -r requirements-dev.txt
maturin build --release --locked --manifest-path sdks/python/Cargo.toml --out target/wheels
python -m pip install target/wheels/partly_sdk-*.whl
export PARTLY_URL=http://127.0.0.1:8080
read -r -s PARTLY_SDK_KEY
export PARTLY_SDK_KEY
python examples/python/rollout.py
```

Paste your copied SDK key when `read` waits for input, then press Enter. This keeps it out of your shell history. The example requires the `new_checkout` flag in the key's environment.

```python
import os
from partly import Client, Decision

client = (
    Client.builder()
    .url("http://127.0.0.1:8080")
    .secret_key(os.environ["PARTLY_SDK_KEY"])
    .build()
)

# Inside your application's async startup/lifecycle:
await client.init()
decision = client.evaluate("new_checkout", context={"id": "user-123"})
if decision is Decision.ENABLED:
    print("Use the new checkout")
changed = await client.refresh()  # False if the policy is unchanged.
await client.aclose()
```

Use `try/finally` for cleanup, as shown in the [complete example](examples/python/rollout.py). Initialization raises on failure. An unknown flag raises `FlagNotFoundError`; it does not silently allow or deny a feature.

## Custom rules

Pass a flat `context` dictionary with the values your flag needs. Percentage rollout uses a stable string `id`; custom rules can use keys such as `age`, `plan`, or `verified`. Context stays inside your application.

```python
decision = client.evaluate(
    "new_checkout",
    context={"id": "user-123", "age": 23, "plan": "pro", "verified": True},
)
```

In the dashboard, every condition uses a key, an operator, and a value. All conditions within a group must match; any group can match. Choose whether matching subjects receive or are excluded from the feature.

| Type | Operators |
| --- | --- |
| Number | `le` (≤), `lt` (<), `ge` (≥), `gt` (>), `eq` (=) |
| String | `prefix`, `suffix`, `contains`, `matches` (exact equality), `regex` |
| Boolean | `is` (true or false) |

String operations are case-sensitive. Types are strict: `"18"` is not a number, and `1` is not a boolean. Missing required fields raise `InvalidContextError`; wrong types raise its subclass `ContextTypeError`, with the flag key, context key, operator, and expected/actual type. The SDK checks every field required by the selected flag before matching groups. The application chooses how to handle these errors; Partly provides no automatic allow/deny fallback.

## Check the POC

With the virtual environment active and the wheel installed:

```sh
python -m playwright install chromium
cargo fmt --all --check
PYO3_PYTHON="$VIRTUAL_ENV/bin/python" cargo test --locked --workspace
PYO3_PYTHON="$VIRTUAL_ENV/bin/python" cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --locked -p partly-server
python -m pytest -q
```

The tests start temporary local servers and databases. They cover the real dashboard, environment navigation, unsaved edits, typed rule inputs, server restarts, key scope, percentage consistency, rule matching, the installed native SDK, refresh failures, async lifecycle, and runnable Python examples. Browser screenshots are written under `test-results/`. To test the release executable, set `PARTLY_BINARY` to its absolute path before running pytest.

Verified on 19 October 2026: 6 Rust tests and 117 Python/end-to-end tests pass against the release executable, including 8 Chromium dashboard tests. The 59 focused SDK tests also pass on Python 3.14. Rust formatting, JavaScript syntax, and Clippy with warnings denied pass. The runnable example exercises 10,000 users.

The independent visual review scored 8.5/10 after two revision retries, with every reviewed screen and state at least 8/10. Generated review reports and screenshots stay outside Git. Firefox, Safari, and real mobile devices were not checked. SQLite and dashboard assets remain bundled in the executable.

## Initial local measurements

Measured on 17 October 2026, macOS 26.6.2 ARM64, Python 3.12.13, with release builds:

| Measurement | Result |
| --- | --- |
| Control-plane executable | 3.75 MiB |
| Compressed native Python wheel | 255 KiB |
| Startup to first status response, median of 5 runs | 11.11 ms |
| Server resident memory after setup and one policy download | 24.28 MiB |
| Full Python `evaluate()` call, median of 5 batches of 100,000 | 0.195 µs |

These historical measurements predate context rules and the Console dashboard; they do not describe the current build. The small baseline used one 50% flag, a fixed user ID, warm operating-system caches, and no application load. Evaluation includes the Python wrapper and native boundary. Memory is one `ps` RSS sample, not a peak measurement. These values are not a load test or a comparison with another tool.

To repeat after building the release executable and installing the release wheel:

```sh
python tests/benchmark.py
```

## Where things live

| Directory | Purpose |
| --- | --- |
| [crates/partly-core](crates/partly-core/README.md) | Policy format, validation, and stable Rust evaluation |
| [crates/partly-server](crates/partly-server/README.md) | Axum server, SQLite, and bundled dashboard |
| [sdks/python](sdks/python/README.md) | Python builder, async lifecycle, and PyO3 binding |
| [examples/python](examples/python/README.md) | Runnable application example used in tests |
| `tests/e2e` | Real server and browser checks |

Development contracts and scope are recorded in [AGENTS.md](AGENTS.md) and the package READMEs.

## Current limits

- One local server, one admin, and one default project. Use localhost; this POC has no TLS or production hosting setup. Direct IP addresses and `localhost` are accepted hostnames.
- SDK keys are stored as plaintext in SQLite, by agreement for this POC. Anyone with the database or a backup can read them. Admin passwords use Argon2id.
- SDK policies live in memory. A running SDK keeps its last good policy during an outage; a new process needs a successful connection before evaluation. Revoking a key stops future downloads, not already loaded rules.
- Polling is included. SSE, HA, RBAC, other languages, persistent SDK snapshots, and licensing are deferred.
- No source license or publication has been selected. Speed and memory goals are not production performance guarantees.
