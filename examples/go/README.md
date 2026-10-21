# Go quickstart

Follow the [server setup](../../README.md#build-locally) to create `new_checkout` and an app that selects it. Replace the URL, environment, application ID, and API Key in [main.go](main.go), then run from the repository root:

```sh
sh sdk/go/build.sh
cd examples/go
go run -a .
```

The `-a` option rebuilds Go code so it links the current Rust library. The example initializes once at application startup, then passes the same client to `checkout` for two example requests. Keep that pointer in application state and reuse it across handlers and goroutines. Only call `Close` at application shutdown; the example also closes before returning an error. See the [SDK guide](../../sdk/go/README.md).

The dashboard serves this same source, and the end-to-end suite runs it for enabled, disabled, and rejected-key cases.
