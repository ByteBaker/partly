# Partly

Partly is a self-hosted feature flag tool. One Rust executable includes the dashboard and SQLite. No Docker, Node.js, or external database is needed to run it.

Manage flags by environment: on, off, percentage rollout, or custom rules. The Python SDK evaluates them locally and keeps working during server outages after its first successful connection.

**Alpha:** `0.1.0-alpha.1` (Python: `0.1.0a1`). Public downloads are not available yet.

## Run locally

Building needs Rust 1.89 or newer and a C compiler. From the repository root:

```sh
cargo build --release --locked -p partly-server
./target/release/partly serve --host 127.0.0.1 --port 8080 --db ./partly.db
```

1. Open <http://127.0.0.1:8080> and create an admin account with a password of at least 12 characters.
2. In **Development**, create a flag named `new_checkout` and choose **Enabled for everyone**.
3. In **Key Management**, generate a named API Key and copy it.

The database is created after setup. Restart with the same path to keep your data. For remote access, follow the [HTTPS setup](crates/partly-server/README.md#https).

## Try Python

Use regular CPython 3.10 or newer. In another terminal, from the repository root:

```sh
python3 -m venv .venv
source .venv/bin/activate
python -m pip install 'maturin>=1.9.4,<2'
maturin build --release --locked --manifest-path sdks/python/Cargo.toml --out target/wheels
python -m pip install target/wheels/partly_sdk-0.1.0a1-*.whl
```

In [quickstart.py](examples/python/quickstart.py), replace `YOUR_API_KEY` with your key. The default URL points to the local server. Then run:

```sh
python examples/python/quickstart.py
```

The example initializes one client, checks `new_checkout`, and closes the client. The [SDK guide](sdks/python/README.md) covers refresh, custom rules, and error handling.

## Alpha limits

- One server and one admin. API Keys are stored as plaintext in SQLite and backups; keep these files private.
- Policies stay in SDK memory. A new application process needs the server to initialize; a running client keeps its last accepted policy during an outage.
- Initial release targets are macOS ARM64 and Linux x86_64. Linux artifacts still need verification. See [release status](RELEASING.md).

## Guides

- [Server: HTTPS, backups, and password recovery](crates/partly-server/README.md)
- [Python SDK and examples](examples/python/README.md)
- [Evaluation rules and percentage consistency](crates/partly-core/README.md)
- [Development checks and release preparation](RELEASING.md)

## License

The control plane and dashboard are source-available under **Elastic License 2.0**. The Python SDK, shared evaluator, and Python examples use **Apache-2.0**. See [licensing details](LICENSING.md).
