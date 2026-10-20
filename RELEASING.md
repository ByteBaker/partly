# Preparing a Partly release

Current candidate: `0.1.0-alpha.1` (Python: `0.1.0a1`). Update the workspace version in `Cargo.toml`, refresh `Cargo.lock`, and build the server and SDK from the same commit.

## Distribution targets

| Artifact | Target | Status |
| --- | --- | --- |
| Server | macOS ARM64 | Packaged HTTPS smoke passed locally |
| Server | Linux x86_64, static musl | No dynamic dependencies; packaged HTTPS smoke passed under emulation |
| Python SDK | macOS ARM64, CPython 3.10+ | Built from source archive; installed-wheel smoke passed |
| Python SDK | Linux x86_64, manylinux2014, CPython 3.10+ | Cross-built from source archive; installed-wheel smoke passed under emulation |
| Rust, Go, Node.js SDKs | macOS ARM64 | Standalone packages passed all 11 shared contract checks |
| Rust, Go, Node.js SDKs | Linux x86_64 | Built and tested outside the checkout under emulation |

ABI3 allows one wheel per platform to support regular CPython 3.10+. CI covers 3.10, 3.12, and 3.14. Windows, Linux ARM64, Intel macOS, Alpine Python, and free-threaded Python are outside this release. The Linux wheel requires glibc; the server is static.

The prebuilt Linux Node.js addon requires glibc 2.34+. Go consumers need cgo and a C compiler. Neither prebuilt package requires a Rust toolchain.

Browser coverage on macOS: the full suite in Chromium, six focused dashboard tests each in Firefox and WebKit. Linux Chromium crashed inside QEMU before running a dashboard test; a native Linux CI run remains required. Safari releases and physical mobile devices are untested.

Local macOS ARM64 checks: 204 Python 3.12/application tests (including all four SDK contracts, examples, and native rebuild regression), 20 Rust tests, 8 Go race tests, and 3 Node.js tests pass. Formatting, Ruff, Clippy, Go vet, dependency audits, package metadata, and release smoke checks pass.

Minimum-version checks: Rust 1.89 builds every workspace target; Go 1.22.12 and Node.js 22.0.0 pass their unit and shared contract tests. CPython 3.10.20 and 3.14.7 each pass 108 SDK/HTTPS tests with the same ABI3 wheel.

Linux x86_64 checks ran on Ubuntu 24.04 under QEMU: 159 backend/Python tests, 20 Rust tests, 8 Go race tests, and 3 Node.js tests pass. The extracted static server and installed manylinux2014 wheel pass the HTTPS release smoke test outside the checkout. Compiler workarounds were needed under emulation; Go vet crashed there and remains covered by macOS and the configured native Linux CI job.

All four packaged SDKs pass their shared contract scenarios on Linux. The Go full-contract test reused the client from its passing refresh-validation test after emulated compiler/linker crashes; the test assertions and Go runtime settings were unchanged.

## Build and verify

Follow the [root setup guide](README.md), then run from the repository root with the virtual environment active. The full suite also needs Go and Node.js (locally tested with Go 1.27.1 and Node.js 26.10.0):

```sh
python -m pip install -r requirements-dev.txt
python -m playwright install chromium
cargo fmt --all --check
python -m ruff check .
PYO3_PYTHON="$VIRTUAL_ENV/bin/python" cargo test --locked --workspace
PYO3_PYTHON="$VIRTUAL_ENV/bin/python" cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --locked -p partly-server -p partly-sdk-contract
sh sdk/go/build.sh
node sdk/node/build.js
(cd sdk/go && go vet ./... && go test -a -race ./...)
node --test sdk/node/test/*.test.js
node --check crates/partly-server/dashboard/app.js
node --check crates/partly-server/dashboard/dropdown.js
python -m pytest -q
```

Tests use temporary servers and databases. `PARTLY_BINARY` selects an absolute executable path. `PARTLY_BROWSER=firefox` or `webkit` selects an installed browser for focused checks. Test reports and screenshots stay outside Git.

The [four-language contract](tests/sdk_contract/README.md) and minimal examples run in the default suite. All must pass before committing. Go links a bundled Rust static library; use `-a` after replacing it because Go's cache does not track archive changes.

Build standalone SDK packages and run the same contracts outside the checkout (Python 3.12+):

```sh
python scripts/package_sdks.py --output target/sdk-packages
PARTLY_SDK_PACKAGES="$PWD/target/sdk-packages" python -m pytest -q tests/e2e/test_sdk_contract.py
```

The output contains two Rust `.crate` files, a Go archive with its static library, and a Node npm tarball with its native extension. Go and Node archives target the build machine; build both macOS ARM64 and Linux x86_64. Consumers do not need Rust for those prebuilt archives. During development only, `--allow-dirty` tests uncommitted packaging changes. Publishing to registries remains a separate step.

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
cargo about generate --locked --offline --fail --config licenses/about.toml --manifest-path sdk/python/Cargo.toml --target x86_64-unknown-linux-gnu --target aarch64-apple-darwin licenses/third-party.hbs --output-file sdk/python/THIRD_PARTY_NOTICES.txt
for language in rust node; do
  cargo about generate --locked --offline --fail --config licenses/about.toml --manifest-path "sdk/$language/Cargo.toml" --target x86_64-unknown-linux-gnu --target aarch64-apple-darwin licenses/third-party.hbs --output-file "sdk/$language/THIRD_PARTY_NOTICES.txt"
done
cargo about generate --locked --offline --fail --config licenses/about.toml --manifest-path crates/partly-ffi/Cargo.toml --target x86_64-unknown-linux-gnu --target aarch64-apple-darwin licenses/third-party.hbs --output-file sdk/go/THIRD_PARTY_NOTICES.txt
```

Review changes and diagnostics: cargo-about 0.9.2 can exit successfully after a clarification checksum mismatch. CI rejects that diagnostic and stale notices. Recheck upstream license checksums in `licenses/about.toml` when upgrading affected dependencies. Generated notices do not replace a review of dependency terms.

Release builds pin Rust 1.98.1. When changing compilers, update the workflow pin and the `RUST_STDLIB_NOTICES.html` files under `sdk/python`, `sdk/go`, and `sdk/node` from that compiler's `share/doc/rust/COPYRIGHT-library.html`. CI verifies an exact match. Local builds also need matching compiler notices before redistribution.

The Linux server archive includes its build runner's musl copyright file; the archive smoke check compares it with that source file. Separately installed Python dependencies carry their own notices.
