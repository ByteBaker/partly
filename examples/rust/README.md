# Rust quickstart

Create `new_checkout` and an API Key in the same environment. Replace the URL and key in [quickstart.rs](quickstart.rs), then run from the repository root:

```sh
cargo run -p partly-rust --example quickstart
```

The example initializes once, evaluates locally, and closes even when evaluation fails. See the [SDK guide](../../sdk/rust/README.md) for refresh, HTTPS, and errors.

The end-to-end suite runs this source for enabled, disabled, and rejected-key cases.
