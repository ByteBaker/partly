# Partly OpenFeature provider for Go

Requires Go 1.26+, the official OpenFeature Go SDK, and the native Partly client. Initialize the Partly client once, register `New(client)`, then reuse the OpenFeature client for local Boolean evaluation.

See the [integration guide](../../../docs/openfeature.md#go) for setup, context mapping, errors, and shutdown. The application owns the borrowed Partly client and must close it at shutdown. This module is included in the platform-specific Partly Go archive.

From the repository root, build the native library with `sh sdk/go/build.sh`, then run `go -C sdk/go/openfeature test -a -race ./...`.

[Apache-2.0](LICENSE).
