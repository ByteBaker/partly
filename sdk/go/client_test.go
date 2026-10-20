// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

package partly

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

const policyJSON = `{"schema_version":1,"project":"default","environment":"development","revision":1,
 "flags":{"flag":{"mode":"custom","action":"enable","groups":[[{"k":"age","op":"eq","v":9007199254740993}]]}}}`

func TestBuilder(t *testing.T) {
	for _, url := range []string{"ftp://host", "http://u:p@host", "http://host/path", "http://host?", "http://host#x", "http://host:99999"} {
		if _, err := NewClientBuilder().URL(url).SecretKey("key").Build(); err == nil {
			t.Fatalf("accepted %s", url)
		}
	}
	if _, err := NewClientBuilder().URL("http://localhost").SecretKey("key\n").Build(); err == nil {
		t.Fatal("accepted invalid key")
	}
	if _, err := NewClientBuilder().URL("http://localhost").SecretKey("key").Timeout(0).Build(); err == nil {
		t.Fatal("accepted zero timeout")
	}
}

func TestNativeNumbersAndStructuredErrors(t *testing.T) {
	snapshot, err := newSnapshot([]byte(policyJSON))
	if err != nil {
		t.Fatal(err)
	}
	for _, value := range []any{int64(9007199254740993), json.Number("9007199254740993")} {
		decision, err := snapshot.evaluate("flag", map[string]any{"age": value})
		if err != nil || decision != Enabled {
			t.Fatalf("precision lost: %v %v", decision, err)
		}
	}
	decision, err := snapshot.evaluate("flag", map[string]any{"age": int64(9007199254740992)})
	if err != nil || decision != Disabled {
		t.Fatalf("%v %v", decision, err)
	}
	_, err = snapshot.evaluate("flag", map[string]any{"age": "private-value"})
	var details *Error
	if !errors.Is(err, ErrContextType) || !errors.Is(err, ErrInvalidContext) || !errors.As(err, &details) ||
		details.K != "age" || details.Op != "eq" || details.ExpectedType != "number" || details.ActualType != "string" {
		t.Fatalf("missing structured error: %#v", err)
	}
	for _, value := range []any{math.NaN(), math.Inf(1), json.Number(""), json.Number("18446744073709551616"), struct{ Age int }{18}} {
		if _, err := snapshot.evaluate("flag", map[string]any{"age": value}); !errors.Is(err, ErrInvalidContext) {
			t.Fatalf("accepted invalid context: %v", err)
		}
	}
}

func TestNativeFloatsKeepTheirValues(t *testing.T) {
	for _, test := range []struct {
		name, op, expected string
		value              any
		want               Decision
	}{
		{"rounded decimal is not an integer", "eq", "1000000000000000100", float64(1000000000000000100), Disabled},
		{"exact float integer value", "eq", "1000000000000000128", float64(1000000000000000100), Enabled},
		{"float32 widens exactly", "gt", "1.20000003", float32(1.2), Enabled},
		{"positive float exceeds integer range", "eq", "1e20", float64(1e20), Enabled},
		{"negative float exceeds integer range", "eq", "-1e20", float64(-1e20), Enabled},
	} {
		t.Run(test.name, func(t *testing.T) {
			policy := fmt.Sprintf(`{"schema_version":1,"project":"default","environment":"development","revision":1,
				"flags":{"flag":{"mode":"custom","action":"enable","groups":[[{"k":"value","op":%q,"v":%s}]]}}}`,
				test.op, test.expected)
			snapshot, err := newSnapshot([]byte(policy))
			if err != nil {
				t.Fatal(err)
			}
			input := map[string]any{"value": test.value}
			got, err := snapshot.evaluate("flag", input)
			if err != nil || got != test.want {
				t.Fatalf("got %q, %v; want %q", got, err, test.want)
			}
			if input["value"] != test.value {
				t.Fatal("evaluation changed the caller's context")
			}
		})
	}
}

type panicMarshaler struct{}

func (panicMarshaler) MarshalJSON() ([]byte, error) { panic("must not marshal unsupported values") }

func TestNativeUnsupportedTypesKeepRuleDetails(t *testing.T) {
	snapshot, err := newSnapshot([]byte(policyJSON))
	if err != nil {
		t.Fatal(err)
	}
	for _, test := range []struct {
		value  any
		actual string
	}{
		{[]int{18}, "array"},
		{[1]int{18}, "array"},
		{map[string]any{"private-value": true}, "object"},
		{struct{ Age int }{18}, "object"},
		{new(int), "object"},
		{panicMarshaler{}, "object"},
		{nil, "null"},
	} {
		_, err := snapshot.evaluate("flag", map[string]any{"age": test.value})
		var details *Error
		if !errors.Is(err, ErrContextType) || !errors.Is(err, ErrInvalidContext) || !errors.As(err, &details) ||
			details.FlagKey != "flag" || details.K != "age" || details.Op != "eq" ||
			details.ExpectedType != "number" || details.ActualType != test.actual {
			t.Fatalf("missing structured error: %#v", err)
		}
		if strings.Contains(err.Error(), "private-value") {
			t.Fatal("error included a context value")
		}
	}
}

func TestNativeRejectsInvalidUTF8(t *testing.T) {
	snapshot, err := newSnapshot([]byte(`{"schema_version":1,"project":"default","environment":"development","revision":1,
		"flags":{"flag":{"mode":"custom","action":"enable","groups":[[{"k":"text","op":"matches","v":"�"}]]}}}`))
	if err != nil {
		t.Fatal(err)
	}
	for _, input := range []map[string]any{
		{"text": "private-value\xff"},
		{"text": "�", "private-key\xff": true},
	} {
		decision, err := snapshot.evaluate("flag", input)
		if decision != "" || !errors.Is(err, ErrInvalidContext) {
			t.Fatalf("accepted invalid UTF-8: %q, %v", decision, err)
		}
		if strings.Contains(err.Error(), "private-") {
			t.Fatal("error included invalid context text")
		}
	}
	if decision, err := snapshot.evaluate("flag", map[string]any{"text": "�"}); err != nil || decision != Enabled {
		t.Fatalf("rejected valid Unicode: %q, %v", decision, err)
	}
}

func TestNativeUnknownFlagPrecedesInvalidContext(t *testing.T) {
	snapshot, err := newSnapshot([]byte(policyJSON))
	if err != nil {
		t.Fatal(err)
	}
	for _, value := range []any{
		[]int{18}, panicMarshaler{}, math.NaN(), json.Number("18446744073709551616"), "private-value\xff",
	} {
		decision, err := snapshot.evaluate("missing", map[string]any{"age": value})
		var details *Error
		if decision != "" || !errors.Is(err, ErrFlagNotFound) || !errors.As(err, &details) || details.FlagKey != "missing" {
			t.Fatalf("invalid context hid unknown flag: %q, %#v", decision, err)
		}
	}
}

type roundTripFunc func(*http.Request) (*http.Response, error)

func (f roundTripFunc) RoundTrip(request *http.Request) (*http.Response, error) { return f(request) }

type cancelAtEOF struct {
	io.Reader
	cancel context.CancelFunc
}

func (r cancelAtEOF) Read(p []byte) (int, error) {
	n, err := r.Reader.Read(p)
	if err == io.EOF {
		r.cancel()
	}
	return n, err
}

func TestCancelledDownloadPreservesPolicyAndETag(t *testing.T) {
	client, err := NewClientBuilder().URL("http://localhost").SecretKey("key").Build()
	if err != nil {
		t.Fatal(err)
	}
	defer client.Close(context.Background())
	client.policy, err = newSnapshot([]byte(policyJSON))
	if err != nil {
		t.Fatal(err)
	}
	accepted := client.policy
	client.etag = `"one"`
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	client.http = &http.Client{Transport: roundTripFunc(func(request *http.Request) (*http.Response, error) {
		if request.Header.Get("If-None-Match") != `"one"` {
			t.Fatal("request lost the accepted ETag")
		}
		return &http.Response{
			StatusCode: 200,
			Header:     http.Header{"Etag": {`"two"`}},
			Body: io.NopCloser(cancelAtEOF{
				Reader: strings.NewReader(strings.Replace(policyJSON, "9007199254740993", "18", 1)),
				cancel: cancel,
			}),
		}, nil
	})}
	if updated, err := client.Refresh(ctx); updated || !errors.Is(err, context.Canceled) {
		t.Fatalf("cancelled download succeeded: %v, %v", updated, err)
	}
	if client.policy != accepted || client.etag != `"one"` || !errors.Is(client.LastRefreshError(), context.Canceled) {
		t.Fatal("cancelled download changed the policy, ETag, or failure diagnostics")
	}
	if decision, err := client.Evaluate("flag", map[string]any{"age": int64(9007199254740993)}); err != nil || decision != Enabled {
		t.Fatalf("cancelled download changed local evaluation: %q, %v", decision, err)
	}
	if updated, err := client.Refresh(context.Background()); !updated || err != nil {
		t.Fatalf("retry failed: %v, %v", updated, err)
	}
	if client.policy == accepted || client.etag != `"two"` || client.LastRefreshError() != nil {
		t.Fatal("retry did not accept the new policy and ETag or clear diagnostics")
	}
}

func TestConcurrentInitEvaluationAndClose(t *testing.T) {
	var requests atomic.Int32
	downloadStarted := make(chan struct{})
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if requests.Add(1) > 1 {
			close(downloadStarted)
			<-r.Context().Done()
			return
		}
		w.Header().Set("ETag", `"one"`)
		_, _ = w.Write([]byte(policyJSON))
	}))
	defer server.Close()
	client, err := NewClientBuilder().URL(server.URL).SecretKey("key").Build()
	if err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	defer client.Close(ctx)
	var wg sync.WaitGroup
	for i := 0; i < 10; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			if err := client.Init(ctx); err != nil {
				t.Error(err)
			}
		}()
	}
	wg.Wait()
	if requests.Load() != 1 {
		t.Fatal("init downloaded more than once")
	}
	refreshDone := make(chan error, 1)
	go func() { _, err := client.Refresh(ctx); refreshDone <- err }()
	select {
	case <-downloadStarted:
	case <-time.After(time.Second):
		t.Fatal("refresh never started")
	}
	for i := 0; i < 10; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			if result, err := client.Evaluate("flag", map[string]any{"age": int64(9007199254740993)}); err != nil || result != Enabled {
				t.Errorf("%v %v", result, err)
			}
		}()
	}
	deadline, cancel := context.WithTimeout(ctx, time.Second)
	defer cancel()
	if err := client.Close(deadline); err != nil {
		t.Fatal(err)
	}
	wg.Wait()
	if err := <-refreshDone; !errors.Is(err, ErrClientClosed) {
		t.Fatalf("refresh: %v", err)
	}
	if err := client.Init(ctx); !errors.Is(err, ErrClientClosed) {
		t.Fatalf("init: %v", err)
	}
}
