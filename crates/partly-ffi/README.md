# Partly native bindings

Shared JSON boundary for Go and Node.js. `partly::evaluator` in the Rust SDK owns all policy validation and evaluation; this crate holds parsed snapshots and maps errors to structured JSON. The dependency disables default features so the native library does not include the Rust HTTP client.

Go links the static library through cgo. Node.js uses its safe Rust API through Node-API. Python uses the same evaluator through PyO3, and the Rust SDK calls it directly.

The C ABI is internal. Callers must keep snapshots alive during evaluation and free returned snapshots and strings exactly once. Go manages these lifetimes. Do not call the C functions directly from applications.

```sh
cargo test -p partly-ffi
cargo build --release --locked -p partly-ffi
```

[Apache-2.0](LICENSE).
