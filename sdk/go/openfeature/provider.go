// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

// Package openfeature adapts an initialized Partly client to the official OpenFeature SDK.
// The application owns the client's initialization and cleanup.
package openfeature

import (
	"context"
	"encoding/json"
	"errors"
	"sync/atomic"

	of "github.com/open-feature/go-sdk/openfeature"
	partly "partly.sh/sdk/go"
)

type Provider struct {
	client *partly.Client
	ready  atomic.Bool
}

func New(client *partly.Client) *Provider { return &Provider{client: client} }
func (p *Provider) Metadata() of.Metadata { return of.Metadata{Name: "Partly"} }
func (p *Provider) Hooks() []of.Hook      { return nil }
func (p *Provider) Init(_ of.EvaluationContext) error {
	p.ready.Store(false)
	if p.client == nil {
		return of.NewProviderNotReadyResolutionError("Supply an initialized Partly client")
	}
	if _, err := p.client.HasFlag(""); err != nil {
		return resolutionError(err).ResolutionError
	}
	p.ready.Store(true)
	return nil
}
func (p *Provider) Shutdown() { p.ready.Store(false) }

func resolutionError(err error) of.ProviderResolutionDetail {
	var value of.ResolutionError
	switch {
	case errors.Is(err, partly.ErrKeyRevoked):
		value = of.NewProviderFatalResolutionError("API Key revoked")
	case errors.Is(err, partly.ErrPolicyNotReady):
		value = of.NewProviderNotReadyResolutionError("Await Partly Init before registering the provider")
	case errors.Is(err, partly.ErrFlagNotFound):
		value = of.NewFlagNotFoundResolutionError("Flag is not defined")
	case errors.Is(err, partly.ErrInvalidContext):
		value = of.NewInvalidContextResolutionError("Context does not satisfy the flag requirements")
	default:
		value = of.NewGeneralResolutionError("Partly evaluation failed")
	}
	return of.ProviderResolutionDetail{ResolutionError: value, Reason: of.ErrorReason}
}

func (p *Provider) check(flag string) error {
	if !p.ready.Load() {
		return partly.ErrPolicyNotReady
	}
	exists, err := p.client.HasFlag(flag)
	if err != nil {
		return err
	}
	if !exists {
		return partly.ErrFlagNotFound
	}
	return nil
}

func (p *Provider) BooleanEvaluation(_ context.Context, flag string, fallback bool, values of.FlattenedContext) of.BoolResolutionDetail {
	if err := p.check(flag); err != nil {
		return of.BoolResolutionDetail{Value: fallback, ProviderResolutionDetail: resolutionError(err)}
	}
	attributes := make(map[string]any, len(values))
	for key, value := range values {
		switch value.(type) {
		case bool, string, int, int8, int16, int32, int64, uint, uint8, uint16, uint32, uint64, float32, float64, json.Number:
			attributes[key] = value
		}
	}
	if key, exists := values[of.TargetingKey]; exists {
		attributes["id"] = key
	}
	decision, err := p.client.Evaluate(flag, attributes)
	if err != nil {
		return of.BoolResolutionDetail{Value: fallback, ProviderResolutionDetail: resolutionError(err)}
	}
	return of.BoolResolutionDetail{Value: decision == partly.Enabled, ProviderResolutionDetail: of.ProviderResolutionDetail{Reason: of.CachedReason, Variant: string(decision)}}
}

func (p *Provider) unsupported(flag string) of.ProviderResolutionDetail {
	if err := p.check(flag); err != nil {
		return resolutionError(err)
	}
	return of.ProviderResolutionDetail{ResolutionError: of.NewTypeMismatchResolutionError("Partly flags are Boolean"), Reason: of.ErrorReason}
}

func (p *Provider) StringEvaluation(_ context.Context, flag string, fallback string, _ of.FlattenedContext) of.StringResolutionDetail {
	return of.StringResolutionDetail{Value: fallback, ProviderResolutionDetail: p.unsupported(flag)}
}
func (p *Provider) FloatEvaluation(_ context.Context, flag string, fallback float64, _ of.FlattenedContext) of.FloatResolutionDetail {
	return of.FloatResolutionDetail{Value: fallback, ProviderResolutionDetail: p.unsupported(flag)}
}
func (p *Provider) IntEvaluation(_ context.Context, flag string, fallback int64, _ of.FlattenedContext) of.IntResolutionDetail {
	return of.IntResolutionDetail{Value: fallback, ProviderResolutionDetail: p.unsupported(flag)}
}
func (p *Provider) ObjectEvaluation(_ context.Context, flag string, fallback any, _ of.FlattenedContext) of.InterfaceResolutionDetail {
	return of.InterfaceResolutionDetail{Value: fallback, ProviderResolutionDetail: p.unsupported(flag)}
}
