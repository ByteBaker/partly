<h1><img src="docs/images/logo.svg" alt="" width="28" height="32"> Partly</h1>

Partly is a self-hosted feature flag tool: one Rust executable with a dashboard and SQLite. No Docker or external services required.

Turn features on or off, roll them out by percentage, or match custom rules. The Python, Rust, Go, and Node.js SDKs evaluate flags locally; user context stays in your application.

**Alpha:** `0.1.0-alpha.1` (Python: `0.1.0a1`). Public downloads are not available yet.

![Partly dashboard showing percentage rollout and custom rules in Development](docs/images/dashboard.png)

## Build locally

Building from source requires Rust 1.89+ and a C compiler. Running the compiled executable needs neither. From the repository root:

```sh
cargo build --release --locked -p partly-server
./target/release/partly serve --host 127.0.0.1 --port 8080 --db ./partly.db
```

1. Open <http://127.0.0.1:8080> and create an admin account (password: at least 12 characters).
2. In **Development**, create `new_checkout` with **Enabled for everyone**.
3. Under **Apps & Keys**, create an app, select its flags, and copy its application ID and API Key.
4. Open **Docs**, choose your language, and copy the complete SDK example.

Setup creates the database. Reuse its path on restart. For remote access, [configure HTTPS](crates/partly-server/README.md#https).

## Connect an application

| Language | Setup and API | Runnable example |
| --- | --- | --- |
| Python (CPython 3.14) | [SDK guide](sdk/python/README.md) | [Quickstart](examples/python/README.md) |
| Rust | [SDK guide](sdk/rust/README.md) | [Quickstart](examples/rust/README.md) |
| Go | [SDK guide](sdk/go/README.md) | [Quickstart](examples/go/README.md) |
| Node.js / TypeScript | [SDK guide](sdk/node/README.md) | [Quickstart](examples/node/README.md) |

Set the server URL, environment, application ID, and API Key. Initialize once at application startup, reuse the client for local evaluations, and close it at shutdown. The dashboard shows these same tested examples, including error handling.

## How it compares

Self-hosted capabilities, reviewed October 2026. Linked sources describe each tool; availability can vary by edition and SDK. **Self-hosted** means your organization runs the control service. **Local evaluation** means flag checks run inside each application; the control service can be on another machine.

| Capability | **Partly** | [Flipt](https://docs.flipt.io/v2/concepts) | [GO Feature Flag](https://gofeatureflag.org/docs/configure_flag/create-flags) | [Unleash](https://docs.getunleash.io/concepts/activation-strategies) | [Flagsmith](https://docs.flagsmith.com/managing-flags/core-management) |
| --- | --- | --- | --- | --- | --- |
| Server language | Rust | Go | Go | TypeScript / Node.js | Python / Django |
| Storage | SQLite | Git | Configuration files | PostgreSQL | PostgreSQL |
| Standalone server binary | ✅ | ✅ | ✅ | ❌ | ❌ |
| Self-hosted central service | ✅ | ✅ | ✅ (relay) | ✅ | ✅ |
| One service for multiple apps | ✅ | ✅ | ✅ | ✅ | ✅ |
| Separate app/project flag collections | ✅ | ✅ [Namespaces](https://docs.flipt.io/v2/concepts#namespaces) | ✅ [Flag sets](https://gofeatureflag.org/docs/concepts/flagset) | ✅ [Projects (Enterprise)](https://docs.getunleash.io/concepts/projects) | ✅ [Projects](https://docs.flagsmith.com/flagsmith-concepts/data-model) |
| Boolean flags | ✅ | ✅ | ✅ | ✅ | ✅ |
| Percentage rollouts | ✅ | ✅ | ✅ | ✅ | ✅ |
| Context-based targeting | ✅ | ✅ | ✅ | ✅ | ✅ |
| Audit logs for flag changes | ✅ | ✅ [Git history](https://docs.flipt.io/v2/introduction#git-native) | ❌ (external history) | ✅ [Basic; advanced in Enterprise](https://docs.getunleash.io/support/oss-comparison#governance-and-security) | ✅ [Audit logs](https://www.flagsmith.com/role-based-access-change-control-security) |
| Non-boolean flag values | ❌ | ✅ | ✅ | ✅ | ✅ |
| Local backend evaluation | ✅ | ✅ | ✅ (by provider) | ✅ | ✅ (opt-in) |

GO Feature Flag relies on the configuration store's history or [change notifications](https://gofeatureflag.org/docs/concepts/notifier) sent to an external system for a retained audit trail.

Deployment details: [Flipt](https://docs.flipt.io/v2/introduction), [GO Feature Flag](https://gofeatureflag.org/docs/concepts/architecture), [Unleash](https://docs.getunleash.io/deploy/configuring-unleash), [Flagsmith](https://docs.flagsmith.com/deployment-self-hosting/hosting-guides/manual-installation).

[Unleash](https://docs.getunleash.io/deploy/getting-started) and [Flagsmith](https://docs.flagsmith.com/flagsmith-concepts/platform-architecture) also offer vendor-hosted control planes. GO Feature Flag's relay serves multiple applications; its Go library can also run directly from configuration without a relay. A shared control plane with local SDK evaluation is a supported pattern across these tools.

### One server, multiple applications

Run Partly once on a shared server with [HTTPS](crates/partly-server/README.md#https). Create an app under **Apps & Keys**, select its flags, and give its SDK the server URL, environment, application ID, and API Key. Each application downloads policy and evaluates flags locally.

```mermaid
flowchart LR
    CP[Partly control plane] -->|Policy download and refresh| A[Billing app: local SDK]
    CP -->|Policy download and refresh| B[API service: local SDK]
    CP -->|Policy download and refresh| C[Worker: local SDK]
```

One instance has Development and Production environments and one admin. Apps select shared flags within their environment; each key can download only its app's selection. Multiple keys support rotation, and revoked keys remain identifiable in history.

Before changing a flag, review the affected apps and type `environment/flag_name` to confirm. The Audits tab records admin changes as readable state transitions. Team permissions and organization-wide capacity testing remain future work.

### SDKs

SDK availability is separate from the server's implementation language:

| SDK | **Partly** | [Flipt](https://github.com/flipt-io/flipt-client-sdks#language-support) | [GO Feature Flag](https://gofeatureflag.org/docs/sdk) | [Unleash](https://docs.getunleash.io/sdks) | [Flagsmith](https://docs.flagsmith.com/integrating-with-flagsmith/sdks/server-side) |
| --- | --- | --- | --- | --- | --- |
| Python | ✅ | ✅ | ✅ | ✅ | ✅ |
| Rust | ✅ | ✅ | ✅ | ✅ | ✅ |
| Go | ✅ | ✅ | ✅ | ✅ | ✅ |
| Node.js / TypeScript | ✅ | ✅ | ✅ | ✅ | ✅ |
| Java | ❌ | ✅ | ✅ | ✅ | ✅ |
| .NET | ❌ | ✅ | ✅ | ✅ | ✅ |
| Ruby | ❌ | ✅ | ✅ | ✅ | ✅ |
| PHP | ❌ | ✅ | ✅ | ✅ | ✅ |
| Browser JavaScript | ❌ | ✅ | ✅ | ✅ | ✅ |
| iOS / Swift | ❌ | ✅ | ✅ | ✅ | ✅ |
| Android / Kotlin | ❌ | ✅ | ✅ | ✅ | ✅ |

GO Feature Flag uses OpenFeature providers or its Go library. Flipt's [Rust and PHP SDKs](https://github.com/flipt-io/flipt-server-sdks#language-support) evaluate remotely. Flagsmith also lists its [browser and mobile SDKs](https://docs.flagsmith.com/integrating-with-flagsmith/sdks#client-side-sdks). SDK availability does not imply local evaluation in every language or runtime.

### Memory

| Memory | **Partly** | [Flipt](https://github.com/flipt-io/helm-charts/blob/main/charts/flipt-v2/values.yaml) | [GO Feature Flag](https://github.com/thomaspoignant/go-feature-flag/blob/main/cmd/relayproxy/helm-charts/relay-proxy/values.yaml) | [Unleash](https://docs.getunleash.io/deploy/configuring-unleash#resource-recommendations) | [Flagsmith](https://docs.flagsmith.com/deployment-self-hosting/scaling-and-performance/sizing-and-scaling#small) |
| --- | --- | --- | --- | --- | --- |
| Server RAM | ~25.5 MiB measured | Not specified | 128 MiB requested by Helm chart | 512 MiB–1 GiB recommended | 2 × 2 GB API workers recommended |
| Additional services | SQLite included | None required | None required | ≥1 GiB for PostgreSQL | 8 GB database + 2 GB task worker; frontend and load balancer also needed |

Flipt leaves resource settings unset; its commented 128 MiB example is not a recommendation. GO Feature Flag's figure is a Kubernetes reservation, not a usage limit. Unleash gives a starting recommendation; Flagsmith's figures describe its Small production tier.

Partly's figure is an earlier baseline of sampled server memory in a one-flag local test, excluding the application SDK and browser. A production RAM recommendation still needs load testing. These figures use different workloads and deployment sizes, so they do not establish a memory ranking.

The same [local benchmark](tests/benchmark.py) measured about **0.32 µs per Python evaluation**, including the Rust binding: five batches of 100,000 calls with one percentage flag and a fixed user. This is a batch average, not tail latency or a cross-product speed comparison.

## Roadmap

Proposed order, keeping the single executable and local evaluation:

1. **Release the existing SDKs.** Finish npm and Go distribution, verify installs from published artifacts, and make the server and four SDKs available through their normal channels. See [publishing work](docs/publishing.md).
2. **Set a tested memory budget.** Measure peak RAM under concurrent logins, policy refreshes, and larger flag sets on Linux. Publish a practical recommendation and track regressions.
3. **Add team access controls.** Application scopes and change history are implemented. Individual admin accounts and permissions are the next step for independent teams sharing one installation.
4. **Support OpenFeature.** Wrap the existing SDKs with providers for the standard feature-flag API, making adoption easier without replacing the Rust evaluator. Keep errors available through the detailed evaluation API.
5. **Add Java, then .NET.** Close the next backend SDK gaps using the shared evaluator and the same cross-language contract tests.
6. **Expand flag values when users need them.** Add string, number, and JSON values before weighted variants, while preserving the existing boolean API. This follows the feature gap in the comparison without adding an experimentation platform.

Browser/mobile SDKs, Ruby/PHP, reusable segments, and scheduled rollouts will be prioritized from user demand. SSO, HA, and experimentation analytics remain outside the near-term scope. The first release stays boolean-only with one admin account.

## Alpha limits

- One server and one admin. API Keys are stored as plaintext in SQLite and backups; keep these files private.
- Policies stay in SDK memory. Running clients keep working during outages; new processes need the server to initialize. Revocation takes effect when the client receives it.
- Release targets: macOS ARM64 and Linux x86_64. See [verified coverage](docs/releasing.md).

## Guides

- [Server: HTTPS, backups, and password recovery](crates/partly-server/README.md)
- [Evaluation rules and percentage consistency](crates/partly-core/README.md)
- [Development checks and release preparation](docs/releasing.md)

## Project layout

```text
crates/     Shared Rust evaluator, native bindings, and control plane
sdk/        Python, Rust, Go, and Node.js SDKs
examples/   Runnable application examples
tests/      Integration, browser, and cross-language tests
config/     Frontend checks and development dependencies
scripts/    Packaging and release checks
docs/       Guides and README images
licenses/   Dependency notices and their generation settings
```

Workspace manifests stay at the root. Python lint, type, and test settings share `pyproject.toml`.

## License

The control plane and dashboard are source-available under **Elastic License 2.0**. The SDKs, shared evaluator, native bindings, and application examples use **Apache-2.0**. See [licensing details](docs/licensing.md).
