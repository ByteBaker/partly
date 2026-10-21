# Rust quickstart

Create `new_checkout` and an API Key in the same environment. Replace the URL, environment, application ID, and key in [quickstart.rs](quickstart.rs), then run from the repository root:

```sh
cargo run -p partly-rust --example quickstart
```

The example initializes once at application startup, then passes the same client to `checkout` for two example requests. The handler only evaluates locally. In a web application, keep the client in application state, using `Arc<Client>` when sharing ownership between tasks. Close it at application shutdown, including after failures. See the [SDK guide](../../sdk/rust/README.md) for refresh, HTTPS, and errors.

The end-to-end suite runs this source for enabled, disabled, and rejected-key cases.

In **Apps & Keys**, create an app in the chosen environment and select `new_checkout`. Copy its application ID and active API Key into the example. Use one initialized client for all requests.
