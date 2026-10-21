# Go quickstart

Create `new_checkout` and an API Key in the same environment. Replace the constants in [main.go](main.go), then run from the repository root:

```sh
sh sdk/go/build.sh
cd examples/go
go run -a .
```

The `-a` option rebuilds Go code so it links the current Rust library. The example initializes once at application startup, then passes the same client to `checkout` for two example requests. Keep that pointer in application state and reuse it across handlers and goroutines. Only call `Close` at application shutdown; the example also closes before returning an error. See the [SDK guide](../../sdk/go/README.md).

The end-to-end suite runs this source for enabled, disabled, and rejected-key cases.

In **Apps & Keys**, create an app in the chosen environment and select `new_checkout`. Copy its application ID and active API Key into the example. Use one initialized client for all requests.
