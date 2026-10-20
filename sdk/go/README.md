# Partly Go SDK

Go handles HTTP and polling; a statically linked Rust library evaluates policies. Reuse one client across goroutines.

## Build locally

Requires Go 1.22+, Rust, and a C compiler. From the repository root:

```sh
cargo build --release --locked -p partly-ffi
cd examples/go
go run -a .
```

First replace the URL and API Key in [the example](../../examples/go/main.go). Its `go.mod` uses a local replacement for this SDK. `partly.sh/sdk/go` is a local module identifier; public module hosting is not configured.

The current cgo build links `target/release/libpartly_ffi.a` from this checkout. Build it for the same machine as the Go application. After rebuilding Rust, use `go build -a`, `go run -a`, or `go test -a` to rebuild Go code: Go's cache does not track changes to this external archive. Prebuilt libraries and standalone module distribution are not yet available.

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

[Apache-2.0](LICENSE). Tested locally on macOS ARM64; Linux checks are configured in CI.
