# Rust quickstart

Follow the [server setup](../../README.md#build-locally) to create `new_checkout` and an app that selects it. Replace the URL, environment, application ID, and API Key in [quickstart.rs](quickstart.rs), then run from the repository root:

```sh
cargo run -p partly-rust --features openfeature --example quickstart
```

The example initializes Partly and registers its OpenFeature provider once at startup. Two requests reuse the OpenFeature client for local checks and handle errors explicitly. Keep both clients in application state and close them at shutdown. See the [SDK guide](../../sdk/rust/README.md).

The dashboard serves this same source, and the end-to-end suite runs it for enabled, disabled, unknown-flag, invalid-context, and rejected-key cases, including installed release packages.

## Typed result example

[typed_results.rs](typed_results.rs) uses OpenFeature's object getter with the same initialize-once lifecycle. Create a `Checkout` record type with one `layout: string` field, then create and assign `checkout_settings`, with `{"layout":"wide"}` selected and `{"layout":"compact"}` otherwise. Set the example's connection constants before running it. [Value types and optional Partly SDK enhancements](../../docs/typed-results.md).
