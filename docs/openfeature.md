# OpenFeature

Use Partly through the official OpenFeature SDK for Python, Rust, Go, or Node.js. These adapters reuse your Partly client's local policy. They do not add a remote evaluation service.

OpenFeature defines the application and provider interfaces, not a policy download format. The server keeps its current JSON policy; the provider maps OpenFeature context into the shared Rust evaluator. Adopting the provider does not change your flags or percentage assignments.

Build the native client with its URL, environment, application ID, and API Key as shown in the [SDK guides](../README.md#connect-an-application). Initialize it **once at application startup**, then register the provider. Keep both clients in application state. Request handlers only evaluate flags; the native client handles polling and manual refresh. The snippets below show where to add OpenFeature to that lifecycle; the runnable quickstarts and dashboard still use the native API.

## Evaluation contract

- Flags remain Boolean. Other typed getters return `TYPE_MISMATCH`; unknown flags return `FLAG_NOT_FOUND`.
- OpenFeature's targeting key maps to Partly's `context.id`, overriding an `id` attribute when both exist. String, number, and Boolean attributes retain their names and types. Structured and datetime fields are ignored; rules requiring one return `INVALID_CONTEXT`.
- Missing or incorrectly typed rule inputs return `INVALID_CONTEXT`. Error messages do not contain context values or credentials.
- Python, Go, and Node return the caller's fallback on errors, with error details. A valid `false` decision is not an error. Rust's upstream SDK returns `Result` instead; handle its error explicitly.
- Ordinary refresh failures keep the accepted policy. Confirmed key revocation makes evaluations fail with `PROVIDER_FATAL` (a general error with that label in Rust).
- Provider registration uses the already downloaded policy. Registering too early reports an initialization error in Python, Go, and Node; Rust exposes an error status and rejects evaluation. Shutting down OpenFeature does not close the borrowed native client; close it separately at application shutdown.

OpenFeature reduces application API changes when switching tools. Flag definitions, targeting rules, and rollout assignments still require migration; percentage cohorts are not portable across vendors.

## Python

Install `partly-sdk[openfeature]` once packages are published. For a local wheel, install it with `openfeature-sdk>=0.10,<0.11` separately. Supports CPython 3.14.

```python
from openfeature import api
from openfeature.evaluation_context import EvaluationContext
from partly.openfeature import PartlyProvider

# Startup: native is a Partly client built with your connection settings.
await native.init()
api.set_provider_and_wait(PartlyProvider(native), "partly")
flags = api.get_client("partly")

# Request handler: reuse flags; this evaluates locally.
result = flags.get_boolean_details(
    "new_checkout", False,
    EvaluationContext(targeting_key="user-123", attributes={"age": 23}),
)
if result.error_code:
    raise RuntimeError(result.error_code)  # Or deliberately use result.value as a fallback.
enabled = result.value

# Shutdown, after requests finish.
api.shutdown()
await native.aclose()
```

Python's provider registration is synchronous, but performs no HTTP work. Both synchronous and asynchronous OpenFeature evaluation methods use the local policy.

## Node.js / TypeScript

Install `@openfeature/server-sdk` alongside `partly-sdk`. The supported peer range is `^1.23.0`; tests pin `1.23.0`. Import the optional adapter from `partly-sdk/openfeature`; native SDK users do not need OpenFeature.

```javascript
const { OpenFeature } = require('@openfeature/server-sdk');
const { PartlyProvider } = require('partly-sdk/openfeature');

// Startup, inside your async application lifecycle.
await native.init();
await OpenFeature.setProviderAndWait('partly', new PartlyProvider(native));
const flags = OpenFeature.getClient('partly');

// Request handler: reuse flags; no HTTP request is made here.
const result = await flags.getBooleanDetails('new_checkout', false, { targetingKey: 'user-123' });
if (result.errorCode) throw new Error(result.errorCode);
const enabled = result.value;

// Shutdown.
await OpenFeature.close();
await native.close();
```

## Go

The adapter is a separate module: `partly.sh/sdk/go/openfeature`. It requires Go 1.26+ for the official OpenFeature Go SDK; the native Partly SDK still supports Go 1.22+. Use a supported, patched Go toolchain.

Until public module hosting is ready, add both replacements to your application, pointing at the extracted Partly Go archive:

```go
require partly.sh/sdk/go/openfeature v0.1.0-alpha.1
replace partly.sh/sdk/go => ./third_party/partly-go
replace partly.sh/sdk/go/openfeature => ./third_party/partly-go/openfeature
```

Use `of "github.com/open-feature/go-sdk/openfeature"` and `provider "partly.sh/sdk/go/openfeature"`:

```go
// Startup. native is a constructed Partly client; ctx is your startup context.
if err := native.Init(ctx); err != nil { return err }
if err := of.SetNamedProviderAndWait("partly", provider.New(native)); err != nil { return err }
flags := of.NewClient("partly")

// Request handler. Return or handle err before using the caller's fallback.
result, err := flags.BooleanValueDetails(ctx, "new_checkout", false, of.NewEvaluationContext("user-123", nil))
if err != nil { return err }
enabled := result.Value

// Shutdown.
of.Shutdown()
if err := native.Close(ctx); err != nil { return err }
```

## Rust

Enable the `openfeature` feature on `partly-rust` and add `open-feature = "0.3"`. The provider borrows an `Arc<partly::Client>`:

```rust
use partly::openfeature::PartlyProvider;
use open_feature::{EvaluationContext, OpenFeature};

// Startup. native is an Arc<Client> built with your connection settings.
native.init().await?;
let mut api = OpenFeature::default();
api.set_provider(PartlyProvider::new(native.clone())).await;
let flags = api.create_client();

// Request handler: propagate or handle the Result; no implicit fallback.
let context = EvaluationContext::default().with_targeting_key("user-123");
let enabled = flags.get_bool_value("new_checkout", Some(&context), None).await?;

// Shutdown.
api.shutdown().await;
native.close().await?;
```

## Coverage and limits

The following baselines follow the [official compatibility table](https://openfeature.dev/docs/reference/sdks/sdk-compatibility/). They describe the versions tested here, not a promise about every later SDK release.

| Language | Tested official SDK | Specification baseline | Result |
| --- | --- | --- | --- |
| Python | `openfeature-sdk` 0.10.0, CPython 3.14 | 0.8.0 | Provider checks below pass |
| Node.js | `@openfeature/server-sdk` 1.23.0 | 0.8.0 | Provider checks below pass |
| Go | `go-sdk` 1.19.0 | 0.7.0 | Provider checks below pass |
| Rust | `open-feature` 0.3.0 | 0.5.2 | Integration checks pass; upstream API differences remain |

This is a tested provider integration, **not certification or a claim of complete specification conformance**. SDK-owned behavior is checked where it connects to Partly; the tests do not replace the upstream SDK conformance suites.

| Contract / specification requirement | Evidence and ownership |
| --- | --- |
| Provider identity (2.1.1) | All adapters expose `Partly`; checked through SDK metadata or hook context |
| Typed resolution (2.2.1–2.2.3) | All official typed getters exist. Known Boolean flags reject other types; unknown flags stay `FLAG_NOT_FOUND`. Rust's trait omits the default argument required by 2.2.1: an upstream difference, not a pass |
| Resolution details and errors (2.2.4–2.2.8) | Tests distinguish a valid `false` from failure, check `enabled`/`disabled` variants and `CACHED`, and check defaults/error codes. Official SDK detail types are reused |
| Flag metadata (2.2.9–2.2.10, later baselines) | Partly adds no optional flag metadata. The official SDK supplies its typed metadata structure |
| Hooks (2.3, section 4) | Provider hook support comes from the official interfaces; Partly adds no provider-owned hooks. Tests exercise application before/after/error/finally hooks and hook context overrides in every language |
| Initialization / shutdown (2.4–2.5, later baselines) | Tests reject uninitialized native clients and reset adapters on shutdown. Native resources remain application-owned. Rust exposes status and drop-based cleanup rather than a provider shutdown callback |
| Context (section 3) | Tests cover global → client → invocation → before-hook precedence, targeting-key precedence, scalar types, invalid required fields, and ignored structured fields |
| Initialization events (5.3, Go/Python/Node baselines) | Tests observe the official SDK's `PROVIDER_ERROR` and `PROVIDER_READY` events when registration fails or succeeds. Rust has no event API |
| Provider isolation / replacement | Named clients keep separate providers; replacement is visible to existing clients without closing borrowed native resources |
| Partly transport and cache contract | Real-server tests cover `304`, changed `200`, outage survival, and cache invalidation after confirmed key revocation through OpenFeature |

Provider-emitted events are optional in [v0.7.0](https://github.com/open-feature/spec/blob/v0.7.0/specification/sections/05-events.md) and [v0.8.0](https://github.com/open-feature/spec/blob/v0.8.0/specification/sections/05-events.md), requirement 5.1.1. The official SDK generates initialization events from the lifecycle result. Partly does not emit refresh, stale, recovery, or post-initialization revocation events; use native refresh errors and evaluation error details. Provider status alone therefore does not report the health of background refresh. Newer v0.9.0 event requirements remain a separate upgrade task.

Rust's upstream API also lacks a dedicated `PROVIDER_FATAL` code and returns `Result` without a caller-default parameter. Partly uses `General("PROVIDER_FATAL")` for confirmed revocation. Initialization cannot return an error through the upstream trait; inspect provider status or handle the evaluation error. These limitations affect the Rust OpenFeature API only; Python, Go, and Node use their own official SDKs over the shared evaluator.

Run the focused checks after installing the native extensions and development dependencies:

```sh
cargo build --locked -p partly-server
python -m pytest -q tests/e2e/test_openfeature.py
cargo test --locked -p partly-rust --features openfeature openfeature::tests
(cd sdk/go/openfeature && go test -race ./...)
node --test sdk/node/test/openfeature.test.js
```

The Python test starts the real server and builds the Rust, Go, and Node contract drivers. The normal build-and-test CI runs these tests. PR verification checks compilation, formatting and types, including the separate Go OpenFeature contract module; it does not build release artifacts. See the [test harness](../tests/sdk_contract/README.md#openfeature-contract).

## Acceptance target

The target remains the smallest supported provider contract with Boolean flags and local evaluation. Optional tracking, static-context reconciliation, remote evaluation, and richer flag types are outside this scope. No OFREP endpoint or alternate server policy format is needed.

Before describing Partly as **OpenFeature-first**, change the runnable quickstarts and dashboard snippets to recommend OpenFeature. Keep native clients available for refresh and cleanup. Track the Rust differences above and review newer specification requirements separately; prefer focused upstream contributions over a replacement SDK.

OpenFeature's [provider conformance assets](https://github.com/open-feature/spec/tree/specification/assets/provider-tck/v0.1.0/specification/assets/provider-tck) are experimental and separate from the SDK test suites. Use their relevant scenarios as additional evidence, with exclusions explained. Their fixture set includes non-Boolean flags, so a selected subset is not a full-suite pass or certification.

References: [OpenFeature providers](https://openfeature.dev/specification/sections/providers/), [Python lifecycle](https://openfeature.dev/docs/reference/sdks/server/python/), and the pinned upstream packages in the lockfiles.
