# Preparing a Partly release

Current candidate: `0.1.0-alpha.1` (Python: `0.1.0a1`). Update the workspace version in `Cargo.toml`, refresh `Cargo.lock`, and build the server and SDK from the same commit.

## Distribution targets

| Artifact | Target | Status |
| --- | --- | --- |
| Server | macOS ARM64 | Packaged HTTPS smoke passed locally |
| Server | Linux x86_64, static musl | Workflow configured; Linux run pending |
| Python SDK | macOS ARM64, CPython 3.10+ | Wheel and source archive built locally |
| Python SDK | Linux x86_64, manylinux2014, CPython 3.10+ | Workflow configured; Linux run pending |

ABI3 allows one wheel per platform to support regular CPython 3.10+. CI covers 3.10, 3.12, and 3.14. Windows, Linux ARM64, Intel macOS, Alpine Python, and free-threaded Python are outside this release. The Linux wheel requires glibc; the server is static.

Browser coverage: the full suite in Chromium, six focused dashboard tests each in Firefox and WebKit. Safari releases and physical mobile devices are untested.

Latest local checks (macOS ARM64): 189 Python 3.12 tests, 13 Rust tests, both focused browser suites, extracted-archive HTTPS smoke, and lints passed. Archive checks rejected missing notices and lost executable permissions. Python 3.10/3.14, dependency audits, and strict package metadata checks need rerunning after the latest test updates. Linux remains unverified.

## Build and verify

Follow the [root setup guide](README.md), then run from the repository root with the virtual environment active. Node.js is needed only for JavaScript syntax checks:

```sh
python -m pip install -r requirements-dev.txt
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

Tests use temporary servers and databases. `PARTLY_BINARY` selects an absolute executable path. `PARTLY_BROWSER=firefox` or `webkit` selects an installed browser for focused checks. Test reports and screenshots stay outside Git.

Optional: `python tests/benchmark.py` measures a release build's size, startup, sampled memory, and Python evaluation including binding overhead. It is not a load test.

[release-checks.yml](.github/workflows/release-checks.yml) runs tests, lints, dependency audits, and packaging checks. It builds the wheel from its source archive, verifies server archive permissions and notices, then tests the extracted server and installed SDK outside the checkout: HTTPS, flags, keys, refresh, and SIGTERM shutdown.

The workflow produces archives, wheels, a source distribution, and `SHA256SUMS`. It does not publish releases or upload to PyPI. Require a green run on the exact release commit.

`scripts/prepare_sdist.py` removes unrelated workspace entries from the archived lockfile, preserving dependency versions and checksums. The repository lockfile stays unchanged; wheels build with `--locked`. This helper requires Python 3.12+.

To check an artifact locally, extract the final server archive and inspect its notices. In a fresh virtual environment outside the checkout, install the exact wheel, `trustme`, and `packaging`. Copy `scripts/release_smoke.py` there and run:

```sh
python -I release_smoke.py /absolute/path/to/partly
```

Run `python -m twine check --strict` on the wheel and source archive. Verify downloads against `SHA256SUMS`; these checksums are not publisher signatures.

## Before public upload

1. Verify packaged licenses, dependency terms, and contributor rights against [LICENSING.md](LICENSING.md).
2. Confirm the public repository and ownership of `partly-sdk` on PyPI.
3. Deploy `https://partly.sh/install` and `https://partly.sh/docs`, or replace these placeholder metadata links.
4. Pass the full workflow on the final commit, resolve advisories, and test the downloaded artifacts.
5. Configure PyPI Trusted Publishing or a scoped credential outside source control.
6. Publish approved archives, wheels, source, checksums, and release notes. Link the [backup and upgrade procedure](crates/partly-server/README.md#backup-restore-and-upgrades).

Signing, notarization, and an automatic installer are not included in this alpha.

## Refresh dependency notices

Notices are tracked. After dependency or target changes, regenerate them from the repository root:

```sh
cargo install cargo-about --version 0.9.2 --locked
cargo fetch --locked
cargo about generate --locked --offline --fail --config licenses/about.toml --manifest-path crates/partly-server/Cargo.toml --target x86_64-unknown-linux-musl --target aarch64-apple-darwin licenses/third-party.hbs --output-file THIRD_PARTY_NOTICES.txt
cargo about generate --locked --offline --fail --config licenses/about.toml --manifest-path sdks/python/Cargo.toml --target x86_64-unknown-linux-gnu --target aarch64-apple-darwin licenses/third-party.hbs --output-file sdks/python/THIRD_PARTY_NOTICES.txt
```

Review changes and diagnostics: cargo-about 0.9.2 can exit successfully after a clarification checksum mismatch. CI rejects that diagnostic and stale notices. Recheck upstream license checksums in `licenses/about.toml` when upgrading affected dependencies. Generated notices do not replace a review of dependency terms.

Release builds pin Rust 1.98.1. When changing compilers, update both the workflow pin and `sdks/python/RUST_STDLIB_NOTICES.html` from that compiler's `share/doc/rust/COPYRIGHT-library.html`. CI verifies an exact match. Local builds also need matching compiler notices before redistribution.

The Linux server archive includes its build runner's musl copyright file. Inspect it when validating the first Linux artifact; macOS checks do not verify it. Separately installed Python dependencies carry their own notices.
