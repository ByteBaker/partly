# Rust quickstart

Follow the [server setup](../../README.md#run-locally) to create `new_checkout` and an app that selects it. Replace the URL, environment, application ID, and API Key in [quickstart.rs](quickstart.rs), then run from the repository root:

```sh
cargo run -p partly-rust --example quickstart
```

The example initializes once at application startup, then passes the same client to `checkout` for two example requests. The handler only evaluates locally. In a web application, keep the client in application state, using `Arc<Client>` when sharing ownership between tasks. Close it at application shutdown, including after failures. See the [SDK guide](../../sdk/rust/README.md) for refresh, HTTPS, and errors.

The dashboard serves this same source, and the end-to-end suite runs it for enabled, disabled, and rejected-key cases.
