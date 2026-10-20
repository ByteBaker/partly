#!/bin/sh
# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Apache-2.0
set -eu

cd "$(dirname "$0")/../.."
if [ "$(go env GOOS)_$(go env GOARCH)" != "$(go env GOHOSTOS)_$(go env GOHOSTARCH)" ]; then
    echo "Build the native library on the target platform; cross-compilation is not configured." >&2
    exit 1
fi
cargo build --locked --release -p partly-ffi --target-dir target
native_dir="sdk/go/native/$(go env GOOS)_$(go env GOARCH)"
mkdir -p "$native_dir"
cp target/release/libpartly_ffi.a "$native_dir/"
