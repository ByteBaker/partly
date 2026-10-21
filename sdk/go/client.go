// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

// Package partly downloads policies and evaluates flags locally through Rust.
package partly

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/url"
	"os"
	"strconv"
	"strings"
	"sync"
	"time"
)

type Decision string

const (
	Enabled  Decision = "enabled"
	Disabled Decision = "disabled"
)

var (
	ErrFlagNotFound   = errors.New("flag not found")
	ErrInvalidContext = errors.New("invalid context")
	ErrContextType    = errors.New("wrong context type")
	ErrPolicyNotReady = errors.New("no policy loaded; call Init first")
	ErrKeyRevoked     = errors.New("API Key revoked; build a new client with a valid key")
	ErrAuthentication = errors.New("API Key rejected")
	ErrInvalidPolicy  = errors.New("invalid policy")
	ErrRefresh        = errors.New("policy request failed")
	ErrClientClosed   = errors.New("client is closed")
)

// Error provides structured context details without including context values.
type Error struct {
	Kind         error
	Message      string
	FlagKey      string
	K            string
	Op           string
	ExpectedType string
	ActualType   string
}

func (e *Error) Error() string { return e.Kind.Error() + ": " + e.Message }
func (e *Error) Unwrap() error { return e.Kind }
func (e *Error) Is(target error) bool {
	return e.Kind == target || (e.Kind == ErrContextType && target == ErrInvalidContext)
}

type ClientBuilder struct {
	url, key, ca, environment, applicationID string
	interval, timeout                        time.Duration
}

func NewClientBuilder() *ClientBuilder {
	return &ClientBuilder{interval: 30 * time.Second, timeout: 5 * time.Second}
}
func (b *ClientBuilder) URL(value string) *ClientBuilder           { b.url = value; return b }
func (b *ClientBuilder) SecretKey(value string) *ClientBuilder     { b.key = value; return b }
func (b *ClientBuilder) Environment(value string) *ClientBuilder   { b.environment = value; return b }
func (b *ClientBuilder) ApplicationID(value string) *ClientBuilder { b.applicationID = value; return b }
func (b *ClientBuilder) CABundle(value string) *ClientBuilder      { b.ca = value; return b }
func (b *ClientBuilder) PollInterval(value time.Duration) *ClientBuilder {
	b.interval = value
	return b
}
func (b *ClientBuilder) Timeout(value time.Duration) *ClientBuilder { b.timeout = value; return b }
func (b *ClientBuilder) Build() (*Client, error) {
	u, err := url.Parse(b.url)
	if err != nil || (u.Scheme != "http" && u.Scheme != "https") || u.Hostname() == "" ||
		u.User != nil || (u.Path != "" && u.Path != "/") || strings.ContainsAny(b.url, "?#\\") ||
		strings.ContainsFunc(b.url, func(r rune) bool { return r <= 32 || r == 127 }) {
		return nil, errors.New("supply an HTTP(S) origin without credentials or a path")
	}
	if u.Port() != "" {
		port, err := strconv.Atoi(u.Port())
		if err != nil || port < 1 || port > 65535 {
			return nil, errors.New("invalid URL port")
		}
	}
	if b.key == "" || strings.ContainsFunc(b.key, func(r rune) bool { return r < 33 || r > 126 }) {
		return nil, errors.New("supply a nonempty ASCII API Key without whitespace")
	}
	for _, value := range []string{b.environment, b.applicationID} {
		if len(value) == 0 || len(value) > 64 || strings.ContainsFunc(value, func(r rune) bool { return !(r >= 'a' && r <= 'z' || r >= '0' && r <= '9' || r == '_' || r == '-') }) {
			return nil, errors.New("supply an environment name and application ID")
		}
	}
	if b.interval <= 0 || b.timeout <= 0 {
		return nil, errors.New("interval and timeout must be positive")
	}
	ctx, cancel := context.WithCancel(context.Background())
	return &Client{config: *b, serial: make(chan struct{}, 1), stopped: ctx, cancel: cancel}, nil
}

// Client is safe for concurrent use. Do not copy it after construction.
type Client struct {
	config    ClientBuilder
	mu        sync.RWMutex
	policy    *snapshot
	etag      string
	closed    bool
	revoked   bool
	lastError error
	http      *http.Client
	serial    chan struct{}
	stopped   context.Context
	cancel    context.CancelFunc
	poller    sync.WaitGroup
}

func (c *Client) acquire(ctx context.Context) error {
	select {
	case c.serial <- struct{}{}:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}
func (c *Client) release() { <-c.serial }
func (c *Client) open() error {
	c.mu.RLock()
	defer c.mu.RUnlock()
	if c.revoked {
		return ErrKeyRevoked
	}
	if c.closed {
		return ErrClientClosed
	}
	return nil
}

func (c *Client) Init(ctx context.Context) error {
	if err := c.acquire(ctx); err != nil {
		return err
	}
	defer c.release()
	if err := c.open(); err != nil {
		return err
	}
	c.mu.RLock()
	ready := c.policy != nil
	c.mu.RUnlock()
	if ready {
		return nil
	}
	if c.http == nil {
		transport := &http.Transport{
			DisableCompression: true,
			ForceAttemptHTTP2:  true,
			IdleConnTimeout:    90 * time.Second,
			TLSClientConfig:    &tls.Config{MinVersion: tls.VersionTLS12},
		}
		if c.config.ca != "" {
			pem, err := os.ReadFile(c.config.ca)
			if err != nil {
				return &Error{Kind: ErrRefresh, Message: "Cannot read CA bundle"}
			}
			roots := x509.NewCertPool()
			if !roots.AppendCertsFromPEM(pem) {
				return &Error{Kind: ErrRefresh, Message: "Invalid CA bundle"}
			}
			transport.TLSClientConfig.RootCAs = roots
		}
		c.http = &http.Client{Timeout: c.config.timeout, Transport: transport,
			CheckRedirect: func(*http.Request, []*http.Request) error { return http.ErrUseLastResponse }}
	}
	if _, err := c.download(ctx); err != nil {
		return err
	}
	c.poller.Add(1)
	go func() {
		defer c.poller.Done()
		timer := time.NewTimer(c.config.interval)
		defer timer.Stop()
		for {
			select {
			case <-c.stopped.Done():
				return
			case <-timer.C:
				_, _ = c.Refresh(c.stopped)
				timer.Reset(c.config.interval)
			}
		}
	}()
	return nil
}

func (c *Client) Refresh(ctx context.Context) (bool, error) {
	if err := c.acquire(ctx); err != nil {
		return false, err
	}
	defer c.release()
	if err := c.open(); err != nil {
		return false, err
	}
	c.mu.RLock()
	ready := c.policy != nil
	c.mu.RUnlock()
	if !ready {
		return false, ErrPolicyNotReady
	}
	return c.download(ctx)
}

func (c *Client) download(ctx context.Context) (updated bool, failure error) {
	defer func() { c.mu.Lock(); c.lastError = failure; c.mu.Unlock() }()
	ctx, cancel := context.WithCancel(ctx)
	stop := context.AfterFunc(c.stopped, cancel)
	defer func() { stop(); cancel() }()
	request, err := http.NewRequestWithContext(ctx, "GET", strings.TrimSuffix(c.config.url, "/")+"/api/policy", nil)
	if err != nil {
		return false, ErrRefresh
	}
	request.Header.Set("Authorization", "Bearer "+c.config.key)
	request.Header.Set("X-Partly-Environment", c.config.environment)
	request.Header.Set("X-Partly-Application", c.config.applicationID)
	c.mu.RLock()
	etag := c.etag
	c.mu.RUnlock()
	if etag != "" {
		request.Header.Set("If-None-Match", etag)
	}
	response, err := c.http.Do(request)
	if err != nil {
		if err := c.open(); err != nil {
			return false, err
		}
		return false, &Error{Kind: ErrRefresh, Message: "Network, timeout, or TLS failure"}
	}
	defer response.Body.Close()
	switch response.StatusCode {
	case 401, 403:
		if response.StatusCode == 401 && response.Header.Get("X-Partly-Error") == "key_revoked" {
			c.mu.Lock()
			c.revoked, c.policy, c.etag = true, nil, ""
			c.cancel()
			c.mu.Unlock()
			return false, ErrKeyRevoked
		}
		return false, ErrAuthentication
	case 304:
		tag := response.Header.Get("ETag")
		if etag == "" || (tag != "" && tag != etag) {
			return false, ErrInvalidPolicy
		}
		return false, nil
	case 200:
	default:
		return false, ErrRefresh
	}
	tag := response.Header.Get("ETag")
	if !validETag(tag) {
		return false, ErrInvalidPolicy
	}
	body, err := io.ReadAll(io.LimitReader(response.Body, 1024*1024+1))
	if err != nil {
		return false, ErrRefresh
	}
	if len(body) > 1024*1024 {
		return false, ErrInvalidPolicy
	}
	policy, err := newSnapshot(body)
	if err != nil {
		return false, err
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.closed {
		return false, ErrClientClosed
	}
	if err := ctx.Err(); err != nil {
		return false, err
	}
	var scope []string
	if json.Unmarshal([]byte(policy.scope), &scope) != nil || len(scope) != 3 || scope[1] != c.config.environment || scope[2] != c.config.applicationID {
		return false, ErrInvalidPolicy
	}
	if c.policy != nil && c.policy.scope != policy.scope {
		return false, ErrInvalidPolicy
	}
	c.policy, c.etag = policy, tag
	return true, nil
}

func validETag(tag string) bool {
	return len(tag) >= 3 && tag[0] == '"' && tag[len(tag)-1] == '"' &&
		!strings.ContainsFunc(tag[1:len(tag)-1], func(r rune) bool { return r < 33 || r > 126 || r == '"' })
}

func (c *Client) Evaluate(flag string, context map[string]any) (Decision, error) {
	c.mu.RLock()
	if c.revoked {
		c.mu.RUnlock()
		return "", ErrKeyRevoked
	}
	policy := c.policy
	c.mu.RUnlock()
	if policy == nil {
		return "", ErrPolicyNotReady
	}
	return policy.evaluate(flag, context)
}
func (c *Client) LastRefreshError() error {
	c.mu.RLock()
	defer c.mu.RUnlock()
	return c.lastError
}
func (c *Client) Close(ctx context.Context) error {
	c.mu.Lock()
	c.closed = true
	c.cancel()
	c.mu.Unlock()
	if err := c.acquire(ctx); err != nil {
		return err
	}
	if c.http != nil {
		c.http.CloseIdleConnections()
	}
	c.release()
	c.poller.Wait()
	return nil
}
