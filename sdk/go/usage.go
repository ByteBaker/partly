// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

package partly

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"strings"
	"time"
)

const usageBatchSize = 128
const usageInterval = 60 * time.Second

// All queue and lifecycle changes share the policy mutex.
type usageState struct {
	queue  []string
	set    map[string]bool
	last   time.Time
	retry  bool
	due    bool
	done   chan struct{}
	cancel context.CancelFunc
}

func (u *usageState) add(flag string) {
	if u.set[flag] {
		return
	}
	if u.set == nil {
		u.set = make(map[string]bool)
	}
	u.set[flag] = true
	u.queue = append(u.queue, flag)
}

func (u *usageState) prune(policy *snapshot) {
	kept := u.queue[:0]
	for _, flag := range u.queue {
		if policy.hasFlag(flag) {
			kept = append(kept, flag)
		} else {
			delete(u.set, flag)
		}
	}
	clear(u.queue[len(kept):])
	u.queue = kept
}

func (u *usageState) take() []string {
	n := min(len(u.queue), usageBatchSize)
	batch := append([]string(nil), u.queue[:n]...)
	for _, flag := range batch {
		delete(u.set, flag)
	}
	copy(u.queue, u.queue[n:])
	clear(u.queue[len(u.queue)-n:])
	u.queue = u.queue[:len(u.queue)-n]
	u.last, u.due = time.Now(), len(u.queue) > 0
	return batch
}

func (c *Client) recordUsageLocked(flag string) {
	u := &c.usage
	u.add(flag)
	if u.done != nil || (time.Since(u.last) < usageInterval && (u.retry || (!u.due && len(u.queue) < usageBatchSize))) {
		return
	}
	batch := u.take()
	done := make(chan struct{})
	ctx, cancel := context.WithCancel(context.Background())
	u.done, u.cancel = done, cancel
	go func() {
		defer func() {
			cancel()
			c.mu.Lock()
			u.done, u.cancel = nil, nil
			close(done)
			c.mu.Unlock()
		}()
		c.reportUsage(ctx, batch)
	}()
}

func (c *Client) reportUsage(ctx context.Context, batch []string) bool {
	body, _ := json.Marshal(struct {
		Flags []string `json:"flags"`
	}{batch})
	request, err := http.NewRequestWithContext(ctx, "POST", strings.TrimSuffix(c.config.url, "/")+"/api/usage", bytes.NewReader(body))
	var response *http.Response
	if err == nil {
		request.Header.Set("Authorization", "Bearer "+c.config.key)
		request.Header.Set("X-Partly-Environment", c.config.environment)
		request.Header.Set("X-Partly-Application", c.config.applicationID)
		request.Header.Set("Content-Type", "application/json")
		response, err = c.http.Do(request)
	}
	if response != nil {
		defer response.Body.Close()
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if err == nil && response.StatusCode == 401 && response.Header.Get("X-Partly-Error") == "key_revoked" {
		c.revokeLocked()
	}
	if c.revoked {
		return false
	}
	success := err == nil && response.StatusCode == http.StatusNoContent
	c.usage.retry = !success
	if !success {
		// Cool down from completion, even when a full batch is waiting.
		c.usage.last = time.Now()
		if !c.closed && c.policy != nil {
			for _, flag := range batch {
				if c.policy.hasFlag(flag) {
					c.usage.add(flag)
				}
			}
		}
	}
	return success
}

func (c *Client) revokeLocked() {
	c.revoked, c.policy, c.etag = true, nil, ""
	c.lastError = ErrKeyRevoked
	c.usage.queue, c.usage.set = nil, nil
	if c.usage.cancel != nil {
		c.usage.cancel()
	}
	c.cancel()
}
