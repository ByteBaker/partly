# Partly Rust SDK

Async policy downloads through Reqwest and rustls; local decisions through `partly-core`. Reuse one client in a Tokio application.

## Use a release archive

Requires Rust 1.89+. Until publication, extract the `partly-core` and `partly-rust` `.crate` archives into `third_party`, then use:

```toml
[dependencies]
partly = { package = "partly-rust", version = "=0.1.0-alpha.1", path = "third_party/partly-rust-0.1.0-alpha.1" }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }

[patch.crates-io]
partly-core = { path = "third_party/partly-core-0.1.0-alpha.1" }
```

The archives build without this checkout. For development inside the repository, use `sdk/rust` as the dependency path.

```rust
use partly::{Client, Context, Decision};

let client = Client::builder()
    .url("http://127.0.0.1:8080")
    .secret_key("YOUR_API_KEY")
    .build()?;
client.init().await?;
let context = Context::from([("id".into(), "user-123".into())]);
let decision = client.evaluate("new_checkout", &context)?;
println!("Enabled: {}", decision == Decision::Enabled);
client.close().await?;
```

The [runnable quickstart](../../examples/rust/quickstart.rs) also closes the client when evaluation fails.

## Client contract

- `build()` validates settings without network access. `init().await` downloads the first policy and starts one poller; failed initialization can be retried.
- Malformed URLs return `Error::UrlParse(UrlParseError)` with the original parser error, also available through `std::error::Error::source()`. Valid URLs that violate the allowed origin format return `Error::Configuration`. Neither error includes the supplied URL or API Key.
- `evaluate()` returns `Result<Decision, Error>`. Match `Error::Evaluation(EvaluationError::ContextType { .. })` for wrong context types. Unknown flags and missing policy are separate errors.
- `refresh().await` returns `true` for an accepted policy, `false` for an unchanged response. Failures preserve the policy and ETag.
- `close().await` cancels downloads and stops polling. Repeated calls are safe. The last policy remains available for evaluation, but the client cannot reconnect.
- `last_refresh_error()` reports a background refresh failure; a successful refresh clears it.

Builder `poll_interval(Duration)` defaults to 30 seconds and `timeout(Duration)` to 5 seconds for the complete HTTP download. Policy validation runs outside the async executor.

HTTPS verifies certificates and hostnames using bundled public roots. `ca_bundle(path)` replaces them with private PEM roots, loaded during initialization. Redirects and environment proxies are disabled. Errors omit request credentials and response bodies.

Context is a flat map of strings, numbers, and booleans. See [shared evaluation rules](../../crates/partly-core/README.md) for types, operators, and stable percentage selection.

```sh
cargo test -p partly-rust
python -m pytest -q tests/e2e/test_sdk_contract.py -k rust
```

[Apache-2.0](LICENSE).
