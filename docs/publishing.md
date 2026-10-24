# Publishing Partly

Partly uses one release version for the server and every SDK. The current candidate is **0.1.0-alpha.1**. Public downloads and registry packages are not available yet.

| Component | Release version | Destination |
| --- | --- | --- |
| Server | `0.1.0-alpha.1`, Git tag `v0.1.0-alpha.1` | Repository release downloads |
| Python `partly-sdk` | `0.1.0a1` | PyPI |
| Rust `partly-core`, `partly-rust` | `0.1.0-alpha.1` | crates.io |
| Go `partly.sh/sdk/go` | `v0.1.0-alpha.1` | Tagged module repository, discovered through Go proxies |
| Node `partly-sdk` | `0.1.0-alpha.1`, npm tag `alpha` | npm |

These are the same release. Python normalizes `alpha.1` to `a1` under [PEP 440](https://packaging.python.org/en/latest/specifications/version-specifiers/#pre-release-spelling); Go requires the leading `v`. The source repository is [ByteBaker/partly](https://github.com/ByteBaker/partly). Package names still need ownership confirmation; PyPI and npm names are independent.

## Keep versions together

Use `Cargo.toml` → `[workspace.package].version` as the source. For the next alpha, change it to `0.1.0-alpha.2`, then update:

- The exact `partly-core` dependency version in the same file.
- `sdk/node/package.json` → `version`.
- The Partly module requirements in `examples/go/go.mod`, `tests/sdk_contract/go/go.mod`, `sdk/go/openfeature/go.mod`, and `tests/sdk_contract/go_openfeature/go.mod`.
- Versioned commands in the READMEs and release guides.

All product Rust crates inherit the workspace version. Maturin derives the Python version from its Rust crate; do not add a second version to `pyproject.toml`. Go modules do not declare their own version in `go.mod`; their release tag supplies it. The private contract-test binary's `0.0.0` is not a product release.

Run `cargo check --workspace` to refresh the workspace entries in `Cargo.lock`, review the diff, then `python -m pytest -q tests/test_release_versions.py`. CI also checks version consistency. Packaging refuses mismatched Node metadata.

Release every component from the same verified commit, including an SDK-only fix. Advance the alpha counter for published fixes; do not overwrite published artifacts or move release tags. A typical sequence is `alpha.1` → `alpha.2` → `beta.1` → `rc.1` → `0.1.0`. Python uses `a1`, `a2`, `b1`, and `rc1` for those stages.

## Before uploading

Complete the [release checks](releasing.md#before-public-upload), confirm package ownership, replace placeholder links, and download the artifacts from the green workflow for the chosen commit. Verify their checksums. Use a clean release staging directory so older builds cannot be uploaded by a wildcard.

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

Confirm ownership of both crate names and sign in with `cargo login`. From the clean release checkout, publish the evaluator first because the SDK depends on it:

```sh
cargo publish --locked --dry-run -p partly-core
cargo publish --locked -p partly-core
# Wait until this version of partly-core is available in the registry.
cargo publish --locked --dry-run -p partly-rust
cargo publish --locked -p partly-rust
```

Only these two crates need publishing. The Python, Go, and Node native binding crates are packaged inside their respective SDK distributions.

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

**Distribution work remains.** Go does not use a PyPI-style upload. It discovers versions through repository tags. For `partly.sh/sdk/go`, the domain must serve `go-import` metadata pointing to the public repository.

The downloaded module must also contain both supported `libpartly_ffi.a` files. They are currently ignored in the development repository and only included in release archives. Merely tagging this checkout would leave normal `go get` consumers unable to link. Keep the [archive plus local replacement](../sdk/go/README.md#use-a-release-archive) installation until this is solved.

To preserve the development repository's no-build-artifacts rule, a dedicated distribution repository can contain the Go SDK and its prebuilt libraries at its root, with `partly.sh/sdk/go` resolving there. In that repository, after testing both platforms:

```sh
git tag v0.1.0-alpha.1
git push origin v0.1.0-alpha.1
GOPROXY=https://proxy.golang.org go list -m partly.sh/sdk/go@v0.1.0-alpha.1
```

The distribution snapshot must come from the same verified Partly source commit. If we instead publish the module from this monorepo's `sdk/go` directory, its tag must be `sdk/go/v0.1.0-alpha.1`; a root `v0.1.0-alpha.1` tag alone does not version that nested module. That route must also include the libraries in the downloadable module.

The optional provider is a separate module, `partly.sh/sdk/go/openfeature`, with the same release version. Include its directory in Go archives. In a distribution repository rooted at the native SDK, its tag is `openfeature/v0.1.0-alpha.1`; in this monorepo it is `sdk/go/openfeature/v0.1.0-alpha.1`. Configure its vanity import path too. Consumers need replacements for both modules until hosting is ready.

Once configured, users run `go get partly.sh/sdk/go@v0.1.0-alpha.1`. Prefer the explicit alpha version; Go normally prefers stable releases when choosing a version. See [publishing Go modules](https://go.dev/doc/modules/publishing), [nested-module tags](https://go.dev/ref/mod#vcs-version), and [prerelease versions](https://go.dev/doc/modules/release-workflow).

## Server and direct SDK downloads

Create a repository release for `v0.1.0-alpha.1`, attach the verified server and SDK archives with their checksum files, and mark it **pre-release**. GitHub provides that setting in its [release UI](https://docs.github.com/en/repositories/releasing-projects-on-github/managing-releases-in-a-repository). It is independent of the PyPI, crates.io, and npm releases.

Our two platform jobs each emit `SHA256SUMS`. Give them platform-specific names, or regenerate and verify one combined checksum file after staging the final assets. Shared artifacts such as the source archive and Rust crates should be attached once. Do not move a prerelease into `latest` or call it stable until the chosen release is ready.
