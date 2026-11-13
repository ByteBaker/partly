// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

module partly-openfeature-contract

go 1.26.0

require (
	github.com/ByteBaker/partly/sdk/go v0.1.0-alpha.1
	github.com/ByteBaker/partly/sdk/go/openfeature v0.1.0-alpha.1
	github.com/open-feature/go-sdk v1.19.0
)

require go.uber.org/mock v0.6.0 // indirect

replace github.com/ByteBaker/partly/sdk/go => ../../../sdk/go

replace github.com/ByteBaker/partly/sdk/go/openfeature => ../../../sdk/go/openfeature
