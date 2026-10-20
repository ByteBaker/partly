# Go quickstart

Create `new_checkout` and an API Key in the same environment. Replace the constants in [main.go](main.go), then run from the repository root:

```sh
cargo build --release --locked -p partly-ffi
cd examples/go
go run -a .
```

The `-a` option rebuilds Go code so it links the current Rust library. The example uses the local SDK, initializes once, evaluates locally, and closes before returning an error. See the [SDK guide](../../sdk/go/README.md).

The end-to-end suite runs this source for enabled, disabled, and rejected-key cases.
