# Partly

Partly is a self-hosted feature flag tool: one Rust executable with a dashboard and SQLite. No Docker or external services required.

Turn features on or off, roll them out by percentage, or match custom rules. The Python SDK evaluates flags locally; user context stays in your application.

**Alpha:** `0.1.0-alpha.1` (Python: `0.1.0a1`). Public downloads are not available yet.

## Run locally

Requires Rust 1.89+ and a C compiler. From the repository root:

```sh
cargo build --release --locked -p partly-server
./target/release/partly serve --host 127.0.0.1 --port 8080 --db ./partly.db
```

1. Open <http://127.0.0.1:8080> and create an admin account (password: at least 12 characters).
2. In **Development**, create `new_checkout` with **Enabled for everyone**.
3. Under **Key Management**, generate and copy an API Key.

Setup creates the database. Reuse its path on restart. For remote access, [configure HTTPS](crates/partly-server/README.md#https).

## Try Python

Use regular CPython 3.10 or newer. In another terminal, from the repository root:

```sh
python3 -m venv .venv
source .venv/bin/activate
python -m pip install 'maturin>=1.9.4,<2'
maturin build --release --locked --manifest-path sdk/python/Cargo.toml --out target/wheels
python -m pip install target/wheels/partly_sdk-0.1.0a1-*.whl
```

Replace `YOUR_API_KEY` in [quickstart.py](examples/python/quickstart.py), then run:

```sh
python examples/python/quickstart.py
```

## Alpha limits

- One server and one admin. API Keys are stored as plaintext in SQLite and backups; keep these files private.
- Policies stay in SDK memory. Running clients keep working during outages; new processes need the server to initialize.
- Release targets: macOS ARM64 and Linux x86_64. Linux artifacts still need verification.

## Guides

- [Server: HTTPS, backups, and password recovery](crates/partly-server/README.md)
- [Python SDK](sdk/python/README.md) and [examples](examples/python/README.md)
- [Evaluation rules and percentage consistency](crates/partly-core/README.md)
- [Development checks and release preparation](RELEASING.md)

## License

The control plane and dashboard are source-available under **Elastic License 2.0**. The Python SDK, shared evaluator, and Python examples use **Apache-2.0**. See [licensing details](LICENSING.md).
