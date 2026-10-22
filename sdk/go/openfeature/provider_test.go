// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

package openfeature

import (
	"context"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"reflect"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	of "github.com/open-feature/go-sdk/openfeature"
	partly "partly.sh/sdk/go"
)

type ageHook struct {
	of.UnimplementedHook
	calls []string
}

func (h *ageHook) Before(_ context.Context, hook of.HookContext, _ of.HookHints) (*of.EvaluationContext, error) {
	h.calls = append(h.calls, "before")
	if hook.EvaluationContext().Attribute("age") != 17 || hook.ProviderMetadata().Name != "Partly" {
		return nil, errors.New("wrong hook context")
	}
	value := of.NewEvaluationContext("", map[string]any{"age": 23})
	return &value, nil
}
func (h *ageHook) After(context.Context, of.HookContext, of.InterfaceEvaluationDetails, of.HookHints) error {
	h.calls = append(h.calls, "after")
	return nil
}
func (h *ageHook) Error(context.Context, of.HookContext, error, of.HookHints) {
	h.calls = append(h.calls, "error")
}
func (h *ageHook) Finally(context.Context, of.HookContext, of.InterfaceEvaluationDetails, of.HookHints) {
	h.calls = append(h.calls, "finally")
}

func TestOfficialSDK(t *testing.T) {
	var requests atomic.Int32
	var revoked atomic.Bool
	var enabled atomic.Bool
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		requests.Add(1)
		if revoked.Load() {
			w.Header().Set("X-Partly-Error", "key_revoked")
			w.WriteHeader(401)
			return
		}
		w.Header().Set("ETag", `"one"`)
		body := `{"schema_version":1,"project":"default","environment":"development","application_id":"app_test","revision":1,"flags":{"off":{"mode":"disabled"},"rollout":{"mode":"percentage","percentage":50},"adult":{"mode":"custom","action":"enable","groups":[[{"k":"age","op":"ge","v":18}]]}}}`
		if enabled.Load() {
			body = strings.Replace(body, "disabled", "enabled", 1)
		}
		_, _ = io.WriteString(w, body)
	}))
	defer server.Close()
	native, err := partly.NewClientBuilder().URL(server.URL).Environment("development").ApplicationID("app_test").SecretKey("key").PollInterval(time.Hour).Build()
	if err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	defer func() { _ = native.Close(ctx) }()
	provider := New(native)
	var initError of.ResolutionError
	if !errors.As(provider.Init(of.NewEvaluationContext("", nil)), &initError) || !strings.HasPrefix(initError.Error(), "PROVIDER_NOT_READY:") {
		t.Fatal("initialization must return a typed OpenFeature error")
	}
	if requests.Load() != 0 {
		t.Fatal("provider initialization made a network request")
	}
	flags := of.NewClient("partly-test")
	events := make(chan of.EventType, 8)
	onReady := func(of.EventDetails) { events <- of.ProviderReady }
	onError := func(of.EventDetails) { events <- of.ProviderError }
	flags.AddHandler(of.ProviderReady, &onReady)
	flags.AddHandler(of.ProviderError, &onError)
	if of.SetNamedProviderAndWait("partly-test", New(native)) == nil {
		t.Fatal("uninitialized provider accepted")
	}
	select {
	case event := <-events:
		if event != of.ProviderError {
			t.Fatal(event)
		}
	case <-time.After(time.Second):
		t.Fatal("no initialization error event")
	}
	if err := native.Init(ctx); err != nil {
		t.Fatal(err)
	}
	if err := of.SetNamedProviderAndWait("partly-test", provider); err != nil {
		t.Fatal(err)
	}
	defer of.Shutdown()
	select {
	case event := <-events:
		if event != of.ProviderReady {
			t.Fatal(event)
		}
	case <-time.After(time.Second):
		t.Fatal("no ready event")
	}
	empty := of.NewEvaluationContext("", nil)
	off, err := flags.BooleanValueDetails(ctx, "off", true, empty)
	if err != nil || off.Value || off.Variant != "disabled" {
		t.Fatalf("false was treated as error: %+v %v", off, err)
	}
	missing, err := flags.BooleanValueDetails(ctx, "missing", true, empty)
	if err == nil || !missing.Value || missing.ErrorCode != of.FlagNotFoundCode {
		t.Fatalf("missing: %+v %v", missing, err)
	}
	text, err := flags.StringValueDetails(ctx, "off", "fallback", empty)
	if err == nil || text.Value != "fallback" || text.ErrorCode != of.TypeMismatchCode {
		t.Fatalf("type mismatch: %+v %v", text, err)
	}
	integer, err := flags.IntValueDetails(ctx, "off", 7, empty)
	if err == nil || integer.Value != 7 || integer.ErrorCode != of.TypeMismatchCode {
		t.Fatalf("integer mismatch: %+v %v", integer, err)
	}
	number, err := flags.FloatValueDetails(ctx, "off", 2.5, empty)
	if err == nil || number.Value != 2.5 || number.ErrorCode != of.TypeMismatchCode {
		t.Fatalf("float mismatch: %+v %v", number, err)
	}
	fallback := map[string]any{"safe": true}
	object, err := flags.ObjectValueDetails(ctx, "off", fallback, empty)
	if err == nil || !reflect.DeepEqual(object.Value, fallback) || object.ErrorCode != of.TypeMismatchCode {
		t.Fatalf("object mismatch: %+v %v", object, err)
	}
	missingText, _ := flags.StringValueDetails(ctx, "missing", "fallback", empty)
	missingInt, _ := flags.IntValueDetails(ctx, "missing", 7, empty)
	missingFloat, _ := flags.FloatValueDetails(ctx, "missing", 2.5, empty)
	missingObject, _ := flags.ObjectValueDetails(ctx, "missing", fallback, empty)
	for _, code := range []of.ErrorCode{missingText.ErrorCode, missingInt.ErrorCode, missingFloat.ErrorCode, missingObject.ErrorCode} {
		if code != of.FlagNotFoundCode {
			t.Fatalf("unknown flag must take precedence over type mismatch: %s", code)
		}
	}
	for _, value := range []any{nil, "private", map[string]any{"nested": 21}} {
		result, err := flags.BooleanValueDetails(ctx, "adult", false, of.NewEvaluationContext("", map[string]any{"age": value}))
		if err == nil || result.ErrorCode != of.InvalidContextCode {
			t.Fatalf("context: %+v %v", result, err)
		}
	}
	result, err := flags.BooleanValueDetails(ctx, "adult", false, of.NewEvaluationContext("", map[string]any{"age": 21, "extra": map[string]any{}}))
	if err != nil || !result.Value {
		t.Fatalf("valid context: %+v %v", result, err)
	}
	selection, err := flags.BooleanValue(ctx, "rollout", false, of.NewEvaluationContext("user-42", map[string]any{"id": "ignored"}))
	decision, nativeErr := native.Evaluate("rollout", map[string]any{"id": "user-42"})
	if err != nil || nativeErr != nil || selection != (decision == partly.Enabled) {
		t.Fatal("targeting key mismatch")
	}
	if requests.Load() != 1 {
		t.Fatal("evaluation made a network request")
	}
	of.SetEvaluationContext(of.NewEvaluationContext("", map[string]any{"age": 17}))
	defer of.SetEvaluationContext(empty)
	hooked := of.NewClient("partly-test")
	if value, err := hooked.BooleanValue(ctx, "adult", true, empty); err != nil || value {
		t.Fatal("global context", value, err)
	}
	hooked.SetEvaluationContext(of.NewEvaluationContext("", map[string]any{"age": 21}))
	if value, err := hooked.BooleanValue(ctx, "adult", false, empty); err != nil || !value {
		t.Fatal("client context", value, err)
	}
	invocation := of.NewEvaluationContext("", map[string]any{"age": 17, "date": time.Now()})
	if value, err := hooked.BooleanValue(ctx, "adult", true, invocation); err != nil || value {
		t.Fatal("invocation context", value, err)
	}
	hook := &ageHook{}
	hooked.AddHooks(hook)
	if value, err := hooked.BooleanValue(ctx, "adult", false, invocation); err != nil || !value {
		t.Fatal("hook context", value, err)
	}
	if !reflect.DeepEqual(hook.calls, []string{"before", "after", "finally"}) {
		t.Fatal(hook.calls)
	}
	hook.calls = nil
	_, _ = hooked.BooleanValue(ctx, "missing", false, invocation)
	if !reflect.DeepEqual(hook.calls, []string{"before", "error", "finally"}) {
		t.Fatal(hook.calls)
	}
	enabled.Store(true)
	other, err := partly.NewClientBuilder().URL(server.URL).Environment("development").ApplicationID("app_test").SecretKey("key").PollInterval(time.Hour).Build()
	if err != nil {
		t.Fatal(err)
	}
	defer func() { _ = other.Close(ctx) }()
	if err := other.Init(ctx); err != nil {
		t.Fatal(err)
	}
	if err := of.SetNamedProviderAndWait("isolated", New(other)); err != nil {
		t.Fatal(err)
	}
	isolated := of.NewClient("isolated")
	if value, err := isolated.BooleanValue(ctx, "off", false, empty); err != nil || !value {
		t.Fatal("isolated provider", value, err)
	}
	if value, err := flags.BooleanValue(ctx, "off", true, empty); err != nil || value {
		t.Fatal("provider leaked state", value, err)
	}
	if err := of.SetNamedProviderAndWait("isolated", New(native)); err != nil {
		t.Fatal(err)
	}
	if value, err := isolated.BooleanValue(ctx, "off", true, empty); err != nil || value {
		t.Fatal("provider replacement", value, err)
	}
	revoked.Store(true)
	if _, err := native.Refresh(ctx); err == nil {
		t.Fatal("revocation accepted")
	}
	fatal, err := flags.BooleanValueDetails(ctx, "off", false, empty)
	if err == nil || fatal.ErrorCode != of.ProviderFatalCode {
		t.Fatalf("revocation: %+v %v", fatal, err)
	}
	if !errors.As(provider.Init(empty), &initError) || !strings.HasPrefix(initError.Error(), "PROVIDER_FATAL:") {
		t.Fatal("revoked initialization must return PROVIDER_FATAL")
	}
	if !errors.Is(provider.check("off"), partly.ErrPolicyNotReady) {
		t.Fatal("failed initialization left the provider ready")
	}
	provider.Shutdown()
	if provider.check("off") == nil {
		t.Fatal("shutdown provider still ready")
	}
}
