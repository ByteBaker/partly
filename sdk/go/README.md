# Partly Go SDK

Go handles HTTP and polling; a statically linked Rust library evaluates policies. Call `Init` once during application startup, then keep the client in application state and reuse it across handlers and goroutines. Request handlers use the official OpenFeature client for local checks; call `Close` at application shutdown. See the [complete quickstart](../../examples/go/main.go).

## Use a release archive

The OpenFeature integration requires Go 1.26+ and a C compiler with cgo enabled. Native-only use supports Go 1.22+. Extract the archive for your platform into `third_party/partly-go` in your application, then add:

```go
require (
    partly.sh/sdk/go v0.1.0-alpha.1
    partly.sh/sdk/go/openfeature v0.1.0-alpha.1
)
replace partly.sh/sdk/go => ./third_party/partly-go
replace partly.sh/sdk/go/openfeature => ./third_party/partly-go/openfeature
```

The archive includes its Rust library under `native/`; consumers do not need Rust or the Partly checkout. Supported targets are macOS ARM64 and Linux x86_64. Public module hosting is still pending.

Go 1.22 is the compatibility minimum. Use a supported Go release with current security patches for production; CI builds and scans with Go 1.27.3.

## Build from source

The OpenFeature example requires Go 1.26+, Rust, and a C compiler. From the repository root:

```sh
sh sdk/go/build.sh
cd examples/go
go run -a .
```

First replace the URL and API Key in [the example](../../examples/go/main.go). Its `go.mod` replaces both the native SDK and provider modules locally. `partly.sh/sdk/go` is a local module identifier; public module hosting is not configured.

The build copies the Rust library into `sdk/go/native/<os>_<arch>/`. Build it for the same machine as the Go application. After replacing the native library, use `go build -a`, `go run -a`, or `go test -a`: Go's cache does not track changes to this archive.

## Native client reference

Build with `NewClientBuilder().URL(url).Environment(env).ApplicationID(appID).SecretKey(key).Build()`. Construction does not contact the server.

| Method | Behavior |
| --- | --- |
| `Init(ctx)` | Downloads the first policy and starts one poller; safe to retry after failure |
| `Evaluate(flag, map[string]any)` | Returns `Enabled` or `Disabled`, plus a separate error |
| `Refresh(ctx)` | Returns `true` for an accepted update, `false` for an unchanged response |
| `Close(ctx)` | Cancels downloads and stops polling; safe to repeat |
| `LastRefreshError()` | Latest refresh failure, cleared after success |

Network calls use Go's HTTP client and honor context cancellation. They wait in the calling goroutine. Evaluation never waits for a network response, and the client is safe for concurrent use.

Defaults: polling every 30 seconds, 5-second whole-download timeout, 1 MiB policy limit. Builder `PollInterval` and `Timeout` take `time.Duration`. HTTPS verifies certificates and hostnames; `CABundle(path)` replaces system trust roots with private PEM roots. Redirects and environment proxies are disabled.

Use `errors.Is(err, partly.ErrFlagNotFound)` or the other exported `Err*` values. `ErrContextType` also matches `ErrInvalidContext`. Use `errors.As` with `*partly.Error` for `FlagKey`, `K`, `Op`, `ExpectedType`, and `ActualType`. Errors never choose a fallback decision.

Context accepts strings, booleans, standard integer and float types, and `json.Number`. Use integers or `json.Number` to preserve exact large values. Floats must be finite; pointers, structs, custom marshalers, and nested values are rejected. See [shared rules](../../crates/partly-core/README.md).

Ordinary failed refreshes preserve the accepted policy and ETag. Closed clients can still evaluate that policy, but cannot reconnect. Native snapshots are released when no Go references remain.

```sh
cd sdk/go
go test -a -race ./...
```

[Apache-2.0](LICENSE).

## Application scope and revocation

Create an application in **Apps & Keys** and select the flags it can use. Supply its environment and permanent application ID on the builder with `.Environment("development").ApplicationID("YOUR_APPLICATION_ID")`, along with its API Key. All are required; the key must match both scope values. Even the first download is checked against this scope. Removing a selected flag makes it unknown after refresh.

A confirmed revoked-key response clears policy and ETag, stops polling, and makes evaluation, initialization, and refresh return `ErrKeyRevoked`. Build a new client with a valid replacement key. Closing a revoked client does not restore evaluation. Ordinary authentication, network, and validation failures preserve the accepted policy. Offline clients learn about revocation only when a refresh reaches the server.

## OpenFeature

The optional `partly.sh/sdk/go/openfeature` module requires Go 1.26+; the native SDK still supports Go 1.22+. Initialize the native client once, then register `openfeature.New(client)` with the official OpenFeature SDK. Registration makes no network requests; evaluations use the same local policy.

OpenFeature returns your fallback together with an error when evaluation fails. Keep the native client for refresh and close it at shutdown. See the [integration guide](../../docs/openfeature.md#go) and [tested compatibility](../../docs/openfeature.md#coverage-and-limits).
