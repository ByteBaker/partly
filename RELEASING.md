# Preparing a Partly release

The first candidate is `0.1.0-alpha.1`. Maturin converts this to the Python version `0.1.0a1`. Change the workspace version in `Cargo.toml`, refresh `Cargo.lock`, and build both packages from the same commit.

Release builds pin Rust 1.98.1. `sdks/python/RUST_STDLIB_NOTICES.html` is copied unchanged from that toolchain's `share/doc/rust/COPYRIGHT-library.html` and travels with both the server and SDK. CI checks that it matches the selected compiler. When upgrading Rust, update the workflow pin and this notice file together. Local builds with another compiler need that compiler's matching notices before redistribution.

The Linux server archive also includes the musl package's copyright file from its build runner. Inspect that file alongside the static binary when validating the first Linux release; local macOS checks cannot verify that artifact.

This candidate includes native HTTPS, verified async SDK connections, local rule evaluation, the bundled dashboard, login throttling, offline password recovery, and tested stopped-database backups. Schema v1 remains unchanged.

## Distribution targets

| Artifact | Initial target | Verification |
| --- | --- | --- |
| Control plane | macOS ARM64 | Build and packaged HTTPS smoke on the development machine |
| Control plane | Linux x86_64, static MUSL | Build and smoke in the release workflow; Linux runner still required |
| Python SDK | macOS ARM64, regular CPython 3.10+ | ABI3 wheel and source-distribution build |
| Python SDK | Linux x86_64, manylinux2014, regular CPython 3.10+ | Build and smoke in the release workflow; Linux runner still required |

ABI3 shares one wheel across compatible Python versions on one platform. It does not make a macOS wheel usable on Linux. CI checks Python 3.10, 3.12, and 3.14. Windows, Linux ARM64, Intel macOS, Alpine Python, and free-threaded Python are outside the first artifact set. The Linux server itself is static; the Linux Python wheel needs a compatible glibc-based system.

The full browser suite runs in Chromium. Focused Firefox and WebKit tests cover rule editing, keyboard dropdown behavior, environment navigation, unsaved edits, browser history, and dropdown placement. These are not tests of Safari releases or physical mobile devices.

Latest verification on macOS ARM64: 189 Python 3.12 tests, 13 Rust tests, and six focused tests each in Firefox and WebKit passed against the updated release binary. The extracted-server HTTPS smoke passed; archive checks also rejected missing notices and lost executable permissions. Formatting and lints pass. Earlier Python 3.10/3.14 runs each passed 162 SDK/server tests; those versions, dependency audits, and strict package metadata checks were not rerun for this test-coverage update. The Linux workflow has not run yet.

## Build and verify

For local development, follow the [root README](README.md) to build the server and install the SDK. Then, with that virtual environment active, run from the repository root. Node.js is needed only for the JavaScript syntax checks:

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

Tests use temporary servers and databases. Set `PARTLY_BINARY` to an absolute executable path to test a release build. `PARTLY_BROWSER=firefox` or `webkit` selects an installed alternative browser for focused dashboard checks. Reports and screenshots in `test-results/` stay outside Git.

For a small local benchmark, build the release server and wheel, then run `python tests/benchmark.py`. It reports executable size, startup time, sampled memory, and Python evaluation time including the native call. It is not a load test or a comparison with other tools.

`.github/workflows/release-checks.yml` runs on pushes, pull requests, and manual dispatch. It checks formatting, lints, Rust and Python tests, dependency advisories, and browser behavior before building artifacts. The artifact jobs build the wheel from its source distribution and check package metadata. They extract the final server archive, verify its executable permission and bundled notices, and run that executable with the installed SDK in a clean environment outside the checkout. That smoke test uses verified HTTPS, creates a flag and key, checks refresh, and stops the server with SIGTERM.

The workflow uploads archives, wheels, the source distribution, and `SHA256SUMS` as workflow artifacts. It does not create a GitHub release, upload to PyPI, or use publishing credentials. A workflow file is not proof of a successful Linux build: require a green run on the exact release commit before distributing its artifacts.

Maturin removes unrelated workspace members from its source archive but keeps their lockfile entries. `scripts/prepare_sdist.py` prunes that archived lockfile offline and verifies that every retained dependency version and checksum is unchanged. The wheel then builds from the extracted archive with `--locked`. The repository lockfile is never modified by this step. Both the release helper and artifact job use Python 3.12+.

To check a built package locally, extract the final server archive and verify its notices. Create a fresh virtual environment outside the checkout, install the exact wheel plus `trustme` and `packaging`, copy `scripts/release_smoke.py` there, and run against the extracted executable:

```sh
python -I release_smoke.py /absolute/path/to/partly
```

Use `python -m twine check --strict` on the wheel and source archive. Inspect `SHA256SUMS` after downloading build artifacts. Checksums detect changed downloads; they are not a publisher signature.

## Before public upload

1. Check the packaged license texts and notices against [LICENSING.md](LICENSING.md): ELv2 for the server, Apache-2.0 for the SDK/core/examples, and original terms for dependencies. Review dependency obligations and contributor rights before public distribution.
2. Confirm the public source repository. The package name is `partly-sdk`; verify control of that PyPI project before upload.
3. Deploy the planned `https://partly.sh/install` and `https://partly.sh/docs` destinations, or replace their metadata links with working pages. They are placeholders today.
4. Run the complete workflow on the final commit. Resolve failed checks and advisories, then test the downloaded artifacts, not a local replacement build.
5. Use PyPI Trusted Publishing or a scoped publishing credential for the confirmed repository. Keep credentials outside source control. Publish only the approved artifacts.
6. Publish the binary archives, checksums, source, and release notes together. Document the [backup and upgrade procedure](crates/partly-server/README.md#backup-restore-and-upgrades).

No public uploads or tags are made by the current workflow. Signing/notarization and an automatic installer are not included in this alpha.

## Refresh dependency notices

Dependency notices are tracked so ordinary source builds need no extra license tool. After changing `Cargo.lock` or the target platforms, use `cargo-about` 0.9.2 from the repository root:

```sh
cargo install cargo-about --version 0.9.2 --locked
cargo fetch --locked
cargo about generate --locked --offline --fail --config licenses/about.toml --manifest-path crates/partly-server/Cargo.toml --target x86_64-unknown-linux-musl --target aarch64-apple-darwin licenses/third-party.hbs --output-file THIRD_PARTY_NOTICES.txt
cargo about generate --locked --offline --fail --config licenses/about.toml --manifest-path sdks/python/Cargo.toml --target x86_64-unknown-linux-gnu --target aarch64-apple-darwin licenses/third-party.hbs --output-file sdks/python/THIRD_PARTY_NOTICES.txt
```

Review the generated changes and all diagnostics. This tool version can return success after a clarification checksum mismatch; CI explicitly rejects that diagnostic and stale generated files. Clarifications in `licenses/about.toml` bind composite or otherwise ambiguous licenses to exact upstream file checksums. Recheck them when upgrading those packages. Notices include build dependencies for completeness; separately installed Python dependencies carry their own notices. These checks collect notices and detect changes; they are not a substitute for reviewing new dependency terms.

## Known alpha limits

One admin, one process, and local SQLite storage. API Keys remain readable in the database and its backups. Certificate issuance and renewal are external. A running SDK keeps its last accepted policy during an outage, but a new process needs a successful initial download. There is no HA, RBAC, persistent SDK cache, or commercial feature licensing.
