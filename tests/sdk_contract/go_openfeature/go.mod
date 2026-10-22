// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

module partly-openfeature-contract

go 1.26.0

require (
	github.com/open-feature/go-sdk v1.19.0
	partly.sh/sdk/go v0.1.0-alpha.1
	partly.sh/sdk/go/openfeature v0.1.0-alpha.1
)

require go.uber.org/mock v0.6.0 // indirect

replace partly.sh/sdk/go => ../../../sdk/go

replace partly.sh/sdk/go/openfeature => ../../../sdk/go/openfeature
