# Publishing Partly

Partly uses one release version for the server and every SDK. The current candidate is **0.1.0-alpha.1**. Use [GitHub Releases](https://github.com/ByteBaker/partly/releases) for server binaries, wheels, and standalone SDK archives. No release assets or registry packages have been published yet.

| Component | Release version | Destination |
| --- | --- | --- |
| Server | `0.1.0-alpha.1`, Git tag `v0.1.0-alpha.1` | [GitHub Releases](https://github.com/ByteBaker/partly/releases) |
| Python `partly-sdk` | `0.1.0a1` | PyPI |
| Rust `partly-rust` (includes the evaluator) | `0.1.0-alpha.1` | crates.io |
| Go `github.com/ByteBaker/partly/sdk/go` | `v0.1.0-alpha.1` | GitHub release archives; direct module installation pending |
| Node `partly-sdk` | `0.1.0-alpha.1`, npm tag `alpha` | npm |

These are the same release. Python normalizes `alpha.1` to `a1` under [PEP 440](https://packaging.python.org/en/latest/specifications/version-specifiers/#pre-release-spelling); Go requires the leading `v`. The source repository is [ByteBaker/partly](https://github.com/ByteBaker/partly). Package names still need ownership confirmation; PyPI and npm names are independent.

## Keep versions together

Use `Cargo.toml` → `[workspace.package].version` as the source. For the next alpha, change it to `0.1.0-alpha.2`, then update:

- The exact `partly` workspace dependency version in the same file.
- `sdk/node/package.json` → `version`.
- The Partly module requirements in `examples/go/go.mod`, `tests/sdk_contract/go/go.mod`, `sdk/go/openfeature/go.mod`, and `tests/sdk_contract/go_openfeature/go.mod`.
- Versioned commands in the READMEs and release guides.

All product Rust crates inherit the workspace version. Maturin derives the Python version from its Rust crate; do not add a second version to `pyproject.toml`. Go modules do not declare their own version in `go.mod`; their release tag supplies it. The private contract-test binary's `0.0.0` is not a product release.

Run `cargo check --workspace` to refresh the workspace entries in `Cargo.lock`, review the diff, then `python -m pytest -q tests/test_release_versions.py`. CI also checks version consistency. Packaging refuses mismatched Node metadata.

Release every component from the same verified commit, including an SDK-only fix. Advance the alpha counter for published fixes; do not overwrite published artifacts or move release tags. A typical sequence is `alpha.1` → `alpha.2` → `beta.1` → `rc.1` → `0.1.0`. Python uses `a1`, `a2`, `b1`, and `rc1` for those stages.

## Before uploading

Complete the [release checks](releasing.md#before-public-upload), confirm package ownership, and download the artifacts from the green workflow for the chosen commit. Verify their checksums. Use a clean release staging directory so older builds cannot be uploaded by a wildcard.

Before adding credentials or a publish job, configure [CI approvals and the protected release environment](releasing.md#ci-security-and-approvals). PR checks must stay separate from publishing permissions and secrets.

Use a versioned GitHub release for the alpha; [prereleases cannot be marked latest](https://docs.github.com/en/rest/releases/releases#create-a-release), so do not rely on `/releases/latest`. After publication, an asset URL has the form `https://github.com/ByteBaker/partly/releases/download/v0.1.0-alpha.1/<asset-name>`. The release workflow currently uploads Actions artifacts only; those still need to be attached to a release after verification.

All Partly source, documentation, and download links use GitHub. Go module paths use `github.com/ByteBaker/partly/sdk/go` and its `/openfeature` submodule. No custom domain or DNS setup is required.

The commands below are instructions, not actions performed by CI. No release packages have been published. Account credentials belong outside the repository.

OpenFeature ships as an optional Python extra, Rust Cargo feature, Node entry point with an optional peer dependency, and a separate Go module. Include these adapters in the same release; see the [tested versions and limits](openfeature.md#coverage-and-limits). Release CI runs the native and OpenFeature contracts and runnable quickstarts against installed wheel/tarball packages and extracted Rust/Go archives. Require these checks on the final artifacts for each supported platform.

## Python: PyPI

Create a PyPI account and confirm the `partly-sdk` name. For a manual first release, create an API token; for automated releases, configure [Trusted Publishing](https://docs.pypi.org/trusted-publishers/).

Put both verified CPython 3.14 wheels and **one** source archive in `release/python/`. Upload only Python distributions, not the server, Rust, Go, or Node archives:

```sh
python -m twine check --strict release/python/*
python -m twine upload release/python/*
```

Twine prompts for the token. PyPI recognizes `0.1.0a1` as a prerelease. Users can install that exact alpha with:

```sh
python -m pip install 'partly-sdk==0.1.0a1'
```

`python -m pip install --pre partly-sdk` also allows prereleases. No separate alpha package is needed. TestPyPI is an optional upload rehearsal, not where real alpha users should install the SDK. See the [upload guide](https://packaging.python.org/en/latest/tutorials/packaging-projects/#uploading-the-distribution-archives) and [pip prerelease behavior](https://pip.pypa.io/en/stable/cli/pip_install/#pre-release-versions).

## Rust: crates.io

Confirm ownership of `partly-rust` and sign in with `cargo login`. The evaluator is a module inside this package. From the clean release checkout:

```sh
cargo publish --locked --dry-run -p partly-rust
cargo publish --locked -p partly-rust
```

Only this crate needs publishing. The Python, Go, and Node native binding crates compile its evaluator with default features disabled and are packaged inside their respective SDK distributions. Release packaging tests the extracted Rust crate without the client feature; the shared SDK contract also tests the complete client and OpenFeature provider outside the checkout.

crates.io accepts `0.1.0-alpha.1` directly. Users opt into this exact alpha in their application's `Cargo.toml`:

```toml
[dependencies]
partly = { package = "partly-rust", version = "=0.1.0-alpha.1" }
```

The `=` pins that build. Without it, Cargo can select a later compatible prerelease. See [publishing crates](https://doc.rust-lang.org/cargo/reference/publishing.html) and [prerelease dependency rules](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#pre-releases).

## Node.js: npm

**Packaging work remains.** The current macOS and Linux tarballs have the same package name and version, but contain different native libraries. npm accepts only one tarball per name/version, so uploading both is not possible. `private: true` currently prevents publishing.

For this alpha, prepare one npm package containing both supported native addons under `native/darwin-arm64/` and `native/linux-x64/`, verify that package on both platforms, and remove `private: true` from its release metadata once the name is confirmed. The existing loader already selects its platform directory. Keep direct platform archives available for local installation. This combined registry package is not built by the current workflow.

After that packaging step, sign in with `npm login` and publish the verified combined tarball:

```sh
npm publish ./release/npm/partly-sdk-0.1.0-alpha.1.tgz --tag alpha --access public
```

Users then install either `npm install partly-sdk@alpha` or `npm install partly-sdk@0.1.0-alpha.1`. The `alpha` tag is a movable label; the numbered version identifies one release. Always supply `--tag alpha` so publication does not use the default `latest` tag. See [npm publishing](https://docs.npmjs.com/cli/v11/commands/npm-publish/) and [distribution tags](https://docs.npmjs.com/adding-dist-tags-to-packages/).

## Go: a tagged module

Go needs no registry account. Its module paths point directly to this GitHub repository:

| Module | Path | Tag for a future module release |
| --- | --- | --- |
| Native SDK | `github.com/ByteBaker/partly/sdk/go` | `sdk/go/v0.1.0-alpha.1` |
| OpenFeature provider | `github.com/ByteBaker/partly/sdk/go/openfeature` | `sdk/go/openfeature/v0.1.0-alpha.1` |

The root `v0.1.0-alpha.1` tag identifies the overall GitHub release; it does not version these nested modules. See [Go's module tag rules](https://go.dev/ref/mod#vcs-version).

**Distribution work remains.** The Go SDK links a Rust native library that is included in release archives but excluded from Git. A tag alone would therefore leave `go get` consumers unable to link. For this alpha, download the platform archive from GitHub Releases and use the [documented local replacements](../sdk/go/README.md#use-a-release-archive) for both modules. Keep generated native libraries out of the source repository; direct module installation needs a separate distribution solution before publishing those module tags.

## Server and direct SDK downloads

Create a repository release for `v0.1.0-alpha.1`, attach the verified server and SDK archives with their checksum files, and mark it **pre-release**. GitHub provides that setting in its [release UI](https://docs.github.com/en/repositories/releasing-projects-on-github/managing-releases-in-a-repository). It is independent of the PyPI, crates.io, and npm releases.

Our two platform jobs each emit `SHA256SUMS`. Give them platform-specific names, or regenerate and verify one combined checksum file after staging the final assets. Shared artifacts such as the source archive and Rust crate should be attached once. Do not move a prerelease into `latest` or call it stable until the chosen release is ready.
