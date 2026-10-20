# Partly Go SDK

Go handles HTTP and polling; a statically linked Rust library evaluates policies. Reuse one client across goroutines.

## Use a release archive

Requires Go 1.22+ and a C compiler with cgo enabled. Extract the archive for your platform into `third_party/partly-go` in your application, then add:

```go
require partly.sh/sdk/go v0.1.0-alpha.1
replace partly.sh/sdk/go => ./third_party/partly-go
```

The archive includes its Rust library under `native/`; consumers do not need Rust or the Partly checkout. Supported targets are macOS ARM64 and Linux x86_64. Public module hosting is still pending.

## Build from source

Requires Go 1.22+, Rust, and a C compiler. From the repository root:

```sh
sh sdk/go/build.sh
cd examples/go
go run -a .
```

First replace the URL and API Key in [the example](../../examples/go/main.go). Its `go.mod` uses a local replacement for this SDK. `partly.sh/sdk/go` is a local module identifier; public module hosting is not configured.

The build copies the Rust library into `sdk/go/native/<os>_<arch>/`. Build it for the same machine as the Go application. After replacing the native library, use `go build -a`, `go run -a`, or `go test -a`: Go's cache does not track changes to this archive.

## Client contract

Build with `NewClientBuilder().URL(url).SecretKey(key).Build()`. Construction does not contact the server.

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

Failed refreshes preserve the accepted policy and ETag. Closed clients can still evaluate that policy, but cannot reconnect. Native snapshots are released when no Go references remain.

```sh
cd sdk/go
go test -a -race ./...
```

[Apache-2.0](LICENSE).
