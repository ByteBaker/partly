#!/bin/sh
# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Elastic-2.0
set -eu

# gofmt itself exits successfully when formatting differs.
unformatted=$(gofmt -l "$@")
if [ -n "$unformatted" ]; then
    printf 'Run gofmt on:\n%s\n' "$unformatted" >&2
    exit 1
fi
