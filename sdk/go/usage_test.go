// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

package partly

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"reflect"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

type usageCall struct {
	flags []string
	reply chan int
}

func usagePolicy(count int) string {
	flags := make(map[string]any)
	for i := 0; i < count; i++ {
		flags[fmt.Sprintf("flag_%03d", i)] = map[string]any{"mode": "disabled"}
	}
	flags["context"] = map[string]any{"mode": "percentage", "percentage": 50}
	data, _ := json.Marshal(map[string]any{
		"schema_version": 1, "application_id": "app_test", "project": "default",
		"environment": "development", "revision": 1, "flags": flags,
	})
	return string(data)
}

func usageClient(t *testing.T, count int) (*Client, chan usageCall, *atomic.Value) {
	t.Helper()
	client, err := NewClientBuilder().URL("http://localhost").Environment("development").ApplicationID("app_test").SecretKey("key").PollInterval(time.Hour).Build()
	if err != nil {
		t.Fatal(err)
	}
	policy := new(atomic.Value)
	policy.Store(usagePolicy(count))
	calls := make(chan usageCall, 10)
	client.http = &http.Client{Transport: roundTripFunc(func(r *http.Request) (*http.Response, error) {
		if r.Header.Get("Authorization") != "Bearer key" || r.Header.Get("X-Partly-Environment") != "development" || r.Header.Get("X-Partly-Application") != "app_test" {
			t.Error("missing scoped authentication")
		}
		if r.URL.Path == "/api/policy" {
			return &http.Response{StatusCode: 200, Header: http.Header{"Etag": {`"one"`}}, Body: io.NopCloser(strings.NewReader(policy.Load().(string)))}, nil
		}
		if r.Method != "POST" || r.URL.Path != "/api/usage" || r.Header.Get("Content-Type") != "application/json" {
			t.Errorf("invalid usage request: %s %s", r.Method, r.URL.Path)
		}
		var body struct {
			Flags []string `json:"flags"`
		}
		if err := json.NewDecoder(r.Body).Decode(&body); err != nil || len(body.Flags) > usageBatchSize {
			t.Errorf("invalid usage payload: %v %#v", err, body)
		}
		call := usageCall{flags: body.Flags, reply: make(chan int, 1)}
		calls <- call
		select {
		case status := <-call.reply:
			headers := make(http.Header)
			if status == 401 {
				headers.Set("X-Partly-Error", "key_revoked")
			}
			return &http.Response{StatusCode: status, Header: headers, Body: io.NopCloser(strings.NewReader(""))}, nil
		case <-r.Context().Done():
			return nil, r.Context().Err()
		}
	})}
	if err := client.Init(context.Background()); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = client.Close(context.Background()) })
	return client, calls, policy
}

func nextUsage(t *testing.T, calls <-chan usageCall) usageCall {
	t.Helper()
	select {
	case call := <-calls:
		return call
	case <-time.After(time.Second):
		t.Fatal("usage report did not start")
		return usageCall{}
	}
}

func finishUsage(t *testing.T, client *Client, call usageCall, status int) {
	t.Helper()
	client.mu.RLock()
	done := client.usage.done
	client.mu.RUnlock()
	call.reply <- status
	select {
	case <-done:
	case <-time.After(time.Second):
		t.Fatal("usage report did not finish")
	}
}

func usageDue(client *Client) {
	client.mu.Lock()
	client.usage.last = time.Now().Add(-usageInterval)
	client.mu.Unlock()
}

func TestUsageLazyDedupAndKnownFailures(t *testing.T) {
	client, calls, _ := usageClient(t, 1)
	for i := 0; i < 10; i++ {
		decision, err := client.Evaluate("flag_000", nil)
		if err != nil || decision != Disabled {
			t.Fatalf("disabled decision changed: %v %v", decision, err)
		}
	}
	if _, err := client.Evaluate("context", nil); !errors.Is(err, ErrInvalidContext) {
		t.Fatal(err)
	}
	_, _ = client.HasFlag("context")
	_, _ = client.Evaluate("missing", nil)
	if len(calls) != 0 {
		t.Fatal("reported before threshold")
	}
	usageDue(client)
	_, _ = client.HasFlag("flag_000")
	if len(calls) != 0 {
		t.Fatal("probe triggered a report")
	}
	_, _ = client.Evaluate("flag_000", nil)
	call := nextUsage(t, calls)
	if !reflect.DeepEqual(call.flags, []string{"flag_000", "context"}) {
		t.Fatal(call.flags)
	}
	_, _ = client.Evaluate("flag_000", nil)
	finishUsage(t, client, call, 204)
	client.mu.RLock()
	if !reflect.DeepEqual(client.usage.queue, []string{"flag_000"}) {
		t.Error("acknowledgement erased evaluation during flight", client.usage.queue)
	}
	client.mu.RUnlock()
}

func TestUsageBoundedSingleFlightAndFairQueue(t *testing.T) {
	client, calls, _ := usageClient(t, 300)
	for i := 0; i < 300; i++ {
		_, _ = client.Evaluate(fmt.Sprintf("flag_%03d", i), nil)
	}
	call := nextUsage(t, calls)
	if len(call.flags) != 128 || call.flags[0] != "flag_000" || call.flags[127] != "flag_127" {
		t.Fatal(call.flags)
	}
	var wg sync.WaitGroup
	for i := 0; i < 30; i++ {
		wg.Add(1)
		go func() { defer wg.Done(); _, _ = client.Evaluate("flag_000", nil) }()
	}
	wg.Wait()
	if len(calls) != 0 {
		t.Fatal("more than one report in flight")
	}
	finishUsage(t, client, call, 204)
	if len(calls) != 0 {
		t.Fatal("completion dispatched without evaluation")
	}
	_, _ = client.Evaluate("flag_000", nil)
	call = nextUsage(t, calls)
	if len(call.flags) != 128 || call.flags[0] != "flag_128" {
		t.Fatal("hot flag overtook queued flags", call.flags)
	}
	finishUsage(t, client, call, 204)
	_, _ = client.Evaluate("flag_000", nil)
	client.mu.RLock()
	if client.usage.done != nil {
		t.Error("fresh partial tail reported before the interval")
	}
	client.mu.RUnlock()
	usageDue(client)
	_, _ = client.Evaluate("flag_000", nil)
	call = nextUsage(t, calls)
	if len(call.flags) != 45 || call.flags[0] != "flag_256" || call.flags[44] != "flag_000" {
		t.Fatal("overdue partial tail was not drained fairly", call.flags)
	}
	finishUsage(t, client, call, 204)
}

func TestOverdueUsageTailDoesNotWaitAnotherInterval(t *testing.T) {
	client, calls, _ := usageClient(t, 300)
	for i := 0; i < 300; i++ {
		_, _ = client.Evaluate(fmt.Sprintf("flag_%03d", i), nil)
	}
	finishUsage(t, client, nextUsage(t, calls), 204)
	usageDue(client)
	_, _ = client.Evaluate("flag_000", nil)
	call := nextUsage(t, calls)
	if len(call.flags) != 128 {
		t.Fatal(call.flags)
	}
	finishUsage(t, client, call, 204)
	_, _ = client.Evaluate("flag_000", nil)
	call = nextUsage(t, calls)
	if len(call.flags) != 45 || call.flags[0] != "flag_256" {
		t.Fatal("overdue tail waited for another interval", call.flags)
	}
	finishUsage(t, client, call, 204)
}

func TestUsageFailureBackoffAndSnapshotPruning(t *testing.T) {
	client, calls, policy := usageClient(t, 130)
	for i := 0; i < 130; i++ {
		_, _ = client.Evaluate(fmt.Sprintf("flag_%03d", i), nil)
	}
	call := nextUsage(t, calls)
	failed := call.flags
	finishUsage(t, client, call, 500)
	_, _ = client.Evaluate("flag_000", nil)
	if len(calls) != 0 || client.LastRefreshError() != nil {
		t.Fatal("report failure affected refresh diagnostics or ignored backoff")
	}
	usageDue(client)
	_, _ = client.Evaluate("flag_000", nil)
	call = nextUsage(t, calls)
	if !reflect.DeepEqual(call.flags, failed) {
		t.Fatal("retry lost the original observation order", call.flags)
	}
	policy.Store(usagePolicy(1))
	if _, err := client.Refresh(context.Background()); err != nil {
		t.Fatal(err)
	}
	finishUsage(t, client, call, 500)
	client.mu.RLock()
	if !reflect.DeepEqual(client.usage.queue, []string{"flag_000"}) || len(client.usage.set) != 1 {
		t.Error("removed names survived refresh or failure", client.usage.queue)
	}
	client.mu.RUnlock()
	usageDue(client)
	_, _ = client.Evaluate("flag_000", nil)
	finishUsage(t, client, nextUsage(t, calls), 204)
}

func TestUsageRevocationAndClose(t *testing.T) {
	t.Run("revocation", func(t *testing.T) {
		client, calls, _ := usageClient(t, 1)
		usageDue(client)
		_, _ = client.Evaluate("flag_000", nil)
		finishUsage(t, client, nextUsage(t, calls), 401)
		if _, err := client.Evaluate("flag_000", nil); !errors.Is(err, ErrKeyRevoked) {
			t.Fatal(err)
		}
		if _, err := client.Refresh(context.Background()); !errors.Is(err, ErrKeyRevoked) {
			t.Fatal(err)
		}
		client.mu.RLock()
		defer client.mu.RUnlock()
		if client.policy != nil || client.etag != "" || len(client.usage.queue) != 0 {
			t.Fatal("revocation kept policy or usage")
		}
	})
	t.Run("close cancels without flush", func(t *testing.T) {
		client, calls, _ := usageClient(t, 1)
		usageDue(client)
		_, _ = client.Evaluate("flag_000", nil)
		_ = nextUsage(t, calls)
		_, _ = client.Evaluate("flag_000", nil)
		ctx, cancel := context.WithTimeout(context.Background(), time.Second)
		defer cancel()
		if err := client.Close(ctx); err != nil {
			t.Fatal(err)
		}
		if decision, err := client.Evaluate("flag_000", nil); err != nil || decision != Disabled {
			t.Fatalf("closed cached evaluation: %v %v", decision, err)
		}
		if len(calls) != 0 || len(client.usage.queue) != 0 {
			t.Fatal("close flushed or recorded usage")
		}
	})
}

func TestUsageRevocationCannotBeUndoneByConcurrentRefresh(t *testing.T) {
	client, calls, _ := usageClient(t, 1)
	base := client.http.Transport
	started, release := make(chan struct{}), make(chan struct{})
	client.http.Transport = roundTripFunc(func(r *http.Request) (*http.Response, error) {
		response, err := base.RoundTrip(r)
		if r.Method == "GET" {
			close(started)
			<-release
		}
		return response, err
	})
	finished := make(chan error, 1)
	go func() { _, err := client.Refresh(context.Background()); finished <- err }()
	select {
	case <-started:
	case <-time.After(time.Second):
		t.Fatal("refresh did not start")
	}
	usageDue(client)
	_, _ = client.Evaluate("flag_000", nil)
	finishUsage(t, client, nextUsage(t, calls), 401)
	close(release)
	if err := <-finished; !errors.Is(err, ErrKeyRevoked) {
		t.Fatal(err)
	}
	if client.policy != nil || client.etag != "" || !errors.Is(client.LastRefreshError(), ErrKeyRevoked) {
		t.Fatal("concurrent refresh undid revocation")
	}
}
