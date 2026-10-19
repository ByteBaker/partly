# Preparing a Partly release

The first candidate is `0.1.0-alpha.1`. Maturin converts this to the Python version `0.1.0a1`. Change the workspace version in `Cargo.toml`, refresh `Cargo.lock`, and build both packages from the same commit.

This candidate includes native HTTPS, verified async SDK connections, local rule evaluation, the bundled dashboard, login throttling, offline password recovery, and tested stopped-database backups. Schema v1 remains unchanged.

## Distribution targets

| Artifact | Initial target | Verification |
| --- | --- | --- |
| Control plane | macOS ARM64 | Build and packaged HTTPS smoke on the development machine |
| Control plane | Linux x86_64, static MUSL | Build and smoke in the release workflow; Linux runner still required |
| Python SDK | macOS ARM64, regular CPython 3.10+ | ABI3 wheel and source-distribution build |
| Python SDK | Linux x86_64, manylinux2014, regular CPython 3.10+ | Build and smoke in the release workflow; Linux runner still required |

ABI3 shares one wheel across compatible Python versions on one platform. It does not make a macOS wheel usable on Linux. CI checks Python 3.10, 3.12, and 3.14. Windows, Linux ARM64, Intel macOS, Alpine Python, and free-threaded Python are outside the first artifact set. The Linux server itself is static; the Linux Python wheel needs a compatible glibc-based system.

The full browser suite runs in Chromium. Focused Firefox and WebKit tests cover rule editing, keyboard dropdown behavior, and environment navigation. These are not tests of Safari releases or physical mobile devices.

Local candidate verification on macOS ARM64: 180 Python 3.12 tests, 162 SDK/server tests each on Python 3.10 and 3.14, 13 Rust tests, and three focused tests each in Firefox and WebKit. Formatting, lints, dependency audits, strict package metadata checks, and the isolated HTTPS artifact smoke pass. The Linux workflow has not run yet.

## Build and verify

`.github/workflows/release-checks.yml` runs on pushes, pull requests, and manual dispatch. It checks formatting, lints, Rust and Python tests, dependency advisories, and browser behavior before building artifacts. The artifact jobs build the wheel from its source distribution, check package metadata, and run the packaged binary and installed SDK together in a clean environment outside the checkout. That smoke test uses verified HTTPS, creates a flag and key, checks refresh, and stops the server with SIGTERM.

The workflow uploads archives, wheels, the source distribution, and `SHA256SUMS` as workflow artifacts. It does not create a GitHub release, upload to PyPI, or use publishing credentials. A workflow file is not proof of a successful Linux build: require a green run on the exact release commit before distributing its artifacts.

Maturin removes unrelated workspace members from its source archive but keeps their lockfile entries. `scripts/prepare_sdist.py` prunes that archived lockfile offline and verifies that every retained dependency version and checksum is unchanged. The wheel then builds from the extracted archive with `--locked`. The repository lockfile is never modified by this step. Both the release helper and artifact job use Python 3.12+.

To check a built package locally, create a fresh virtual environment outside the checkout, install the exact wheel plus `trustme` and `packaging`, copy `scripts/release_smoke.py` there, and run:

```sh
python -I release_smoke.py /absolute/path/to/partly
```

Use `python -m twine check --strict` on the wheel and source archive. Inspect `SHA256SUMS` after downloading build artifacts. Checksums detect changed downloads; they are not a publisher signature.

## Before public upload

1. Choose and add the source license, including the SDK and shared evaluator, and review dependency license obligations.
2. Confirm the public source repository. The package name is `partly-sdk`; verify control of that PyPI project before upload.
3. Deploy the planned `https://partly.sh/install` and `https://partly.sh/docs` destinations, or replace their metadata links with working pages. They are placeholders today.
4. Run the complete workflow on the final commit. Resolve failed checks and advisories, then test the downloaded artifacts, not a local replacement build.
5. Use PyPI Trusted Publishing or a scoped publishing credential for the confirmed repository. Keep credentials outside source control. Publish only the approved artifacts.
6. Publish the binary archives, checksums, source, and release notes together. Document the [backup and upgrade procedure](crates/partly-server/README.md#backup-restore-and-upgrades).

No public uploads or tags are made by the current workflow. Signing/notarization and an automatic installer are not included in this alpha.

## Known alpha limits

One admin, one process, and local SQLite storage. API Keys remain readable in the database and its backups. Certificate issuance and renewal are external. A running SDK keeps its last accepted policy during an outage, but a new process needs a successful initial download. There is no HA, RBAC, persistent SDK cache, or commercial feature licensing.
