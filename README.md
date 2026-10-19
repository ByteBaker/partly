# Partly

Partly is a small feature flag control plane. Run one Rust executable, set up an admin account in your browser, and connect a Python application. Flag checks run locally inside the SDK, even when the control plane is offline.

This is an alpha candidate (`0.1.0-alpha.1`; Python package `0.1.0a1`). It supports boolean decisions, stable percentage rollouts, local context rules, development and production environments, and named API Keys. The dashboard and SQLite are included in the executable. Docker, Node.js, and an external database are not needed to run it.

Public distribution is being prepared. The package name is `partly-sdk` on PyPI. `https://partly.sh`, `/install`, and `/docs` are planned destinations, not working download instructions yet. No public upload has been made; the source license remains undecided. See the [release checklist](RELEASING.md).

## Run the control plane

Building from source needs a recent stable Rust toolchain (at least 1.89) and a C compiler. From this directory:

```sh
cargo build --release --locked -p partly-server
./target/release/partly serve --host 127.0.0.1 --port 8080 --db ./partly.db
```

Open <http://127.0.0.1:8080>. Create an admin account with a password of at least 12 characters. Partly creates the database only after valid setup. An existing invalid database causes a startup error; it is never replaced automatically.

For HTTPS, supply `--tls-cert` and `--tls-key`, plus `--hostname` when using a custom DNS name. See the [HTTPS setup](crates/partly-server/README.md#https) for certificate requirements and an example command. The Python SDK uses async HTTPX transport with certificate verification; Rust handles local evaluation.

Choose **Development** in the top-right environment selector. In **Feature flags**, select **Create flag**, enter `new_checkout`, and choose a rollout mode: enabled for everyone, percentage of users, custom rules, or disabled for everyone. Creation always uses the current environment. The environment is also in the URL; switching it reloads the page, with a warning before discarding unsaved edits.

Open **Key Management**, select **Generate key**, enter a name, and generate an API Key. Select **Copy** to use it. The key stays masked; **Reveal** remains available whenever you need to see it. **Docs** contains the complete Python quickstart, with syntax colors, a language dropdown, and a copy icon.

This alpha uses database schema v1. Earlier development databases may be incompatible; use a fresh path to try the alpha without changing the old file. Once released, this v1 format is the compatibility baseline: later changes must preserve user data. There are no migrations in this version, and unsupported files are never reset. See [backup, restore, and upgrades](crates/partly-server/README.md#backup-restore-and-upgrades).

After building, the executable can be moved to another directory and run by itself on the same platform. The Python package is separate.

## Build and try the Python SDK

The SDK targets regular CPython 3.10+ using a native ABI3 extension. Release checks cover Python 3.10, 3.12, and 3.14. The first artifact targets are Linux x86_64 and macOS ARM64; see [verified platforms and limits](RELEASING.md). Free-threaded Python is not supported by this wheel.

```sh
python3 -m venv .venv
source .venv/bin/activate
python -m pip install -r requirements-dev.txt
maturin build --release --locked --manifest-path sdks/python/Cargo.toml --out target/wheels
python -m pip install target/wheels/partly_sdk-0.1.0a1-*.whl
```

In [quickstart.py](examples/python/quickstart.py), replace `YOUR_API_KEY` with your copied API Key and set `CONTROL_PLANE_URL` to the server address. The example requires the `new_checkout` flag in the key's environment. No environment variables are required.

```sh
python examples/python/quickstart.py
```

The [quickstart](examples/python/quickstart.py) builds one client, initializes it, evaluates one flag, and closes it in `finally`. It prints one decision. Initialization raises on failure. An unknown flag raises `FlagNotFoundError`; it does not silently allow or deny a feature. See the [SDK reference](sdks/python/README.md) for refresh and error types, or the [rollout example](examples/python/README.md#check-a-larger-rollout) to check 10,000 users with a compact summary.

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

## Check the application

With the virtual environment active and the wheel installed (Node.js is needed only for the JavaScript syntax check):

```sh
python -m playwright install chromium
cargo fmt --all --check
python -m ruff check .
PYO3_PYTHON="$VIRTUAL_ENV/bin/python" cargo test --locked --workspace
PYO3_PYTHON="$VIRTUAL_ENV/bin/python" cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --locked -p partly-server
node --check crates/partly-server/dashboard/app.js
node --check crates/partly-server/dashboard/dropdown.js
python -m pytest -q
```

The tests start temporary local servers and databases. They cover the real dashboard, environment navigation, unsaved edits, typed rule inputs, server restarts, key scope, percentage consistency, rule matching, the installed native SDK, refresh failures, async lifecycle, and runnable Python examples. Browser screenshots are written under `test-results/`. To test the release executable, set `PARTLY_BINARY` to its absolute path before running pytest.

HTTPS tests generate temporary certificates with the development-only `trustme` package. They check TLS 1.2/1.3, HTTP/2 dashboard access, secure cookies, private CA trust, invalid certificates, and policy retention after TLS failures. They need no external certificate service.

The full browser suite uses Chromium at desktop and mobile viewport sizes. Focused Firefox and WebKit checks cover rule editing, dropdown keyboard controls, and environment navigation. Set `PARTLY_BROWSER=firefox` or `webkit` for those dashboard tests. Safari itself and real mobile devices are not verified. Generated reports, screenshots, databases, and caches stay outside Git.

## Measure locally

After building the release executable and installing the release wheel:

```sh
python tests/benchmark.py
```

This reports executable size, startup time, server memory, and full Python `evaluate()` time, including the native boundary. It uses five startup samples and five batches of 100,000 evaluations with one percentage flag and a fixed context ID. Memory is one `ps` RSS sample, not a peak. This small macOS/Linux benchmark is not a load test or a comparison with other tools.

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

- One server, one admin, and one default project. Native HTTPS uses supplied certificate/key files; see the [HTTPS setup](crates/partly-server/README.md#https). Direct IPs, `localhost`, and one configured DNS hostname are accepted. Certificate issuance, renewal, and reverse-proxy TLS termination remain outside this setup.
- API Keys are stored as plaintext in SQLite, by agreement for this POC. Anyone with the database or a backup can read them. Admin passwords use Argon2id.
- SDK policies live in memory. A running SDK keeps its last good policy during an outage; a new process needs a successful connection before evaluation. Revoking a key stops future downloads, not already loaded rules.
- Polling is included. SSE, HA, RBAC, other languages, persistent SDK snapshots, and licensing are deferred.
- No source license or publication has been selected. Speed and memory goals are not production performance guarantees.
