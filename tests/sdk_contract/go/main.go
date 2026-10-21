// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

package main

import (
	"bufio"
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"time"

	partly "partly.sh/sdk/go"
)

type command struct {
	Environment   string         `json:"environment"`
	ApplicationID string         `json:"application_id"`
	Op            string         `json:"op"`
	URL           string         `json:"url"`
	Key           string         `json:"key"`
	CABundle      string         `json:"ca_bundle"`
	Interval      float64        `json:"interval"`
	Timeout       float64        `json:"timeout"`
	Flag          string         `json:"flag"`
	Context       map[string]any `json:"context"`
}

func errorCode(err error) string {
	switch {
	case errors.Is(err, partly.ErrFlagNotFound):
		return "flag_not_found"
	case errors.Is(err, partly.ErrContextType):
		return "context_type"
	case errors.Is(err, partly.ErrInvalidContext):
		return "invalid_context"
	case errors.Is(err, partly.ErrPolicyNotReady):
		return "policy_not_ready"
	case errors.Is(err, partly.ErrKeyRevoked):
		return "key_revoked"
	case errors.Is(err, partly.ErrAuthentication):
		return "authentication"
	case errors.Is(err, partly.ErrInvalidPolicy):
		return "invalid_policy"
	case errors.Is(err, partly.ErrRefresh):
		return "refresh"
	case errors.Is(err, partly.ErrClientClosed):
		return "client_closed"
	default:
		panic(err)
	}
}

func execute(client **partly.Client, input command) (map[string]any, error) {
	ctx := context.Background()
	result := map[string]any{"ok": true}
	var err error
	switch input.Op {
	case "build":
		builder := partly.NewClientBuilder().URL(input.URL).Environment(input.Environment).ApplicationID(input.ApplicationID).SecretKey(input.Key).
			PollInterval(time.Duration(input.Interval * float64(time.Second))).
			Timeout(time.Duration(input.Timeout * float64(time.Second)))
		if input.CABundle != "" {
			builder = builder.CABundle(input.CABundle)
		}
		*client, err = builder.Build()
	case "init":
		err = (*client).Init(ctx)
	case "evaluate":
		var decision partly.Decision
		decision, err = (*client).Evaluate(input.Flag, input.Context)
		if err == nil {
			switch decision {
			case partly.Enabled:
				result = map[string]any{"decision": "enabled"}
			case partly.Disabled:
				result = map[string]any{"decision": "disabled"}
			default:
				panic("SDK returned an unknown decision")
			}
		}
	case "refresh":
		var updated bool
		updated, err = (*client).Refresh(ctx)
		result = map[string]any{"updated": updated}
	case "close":
		err = (*client).Close(ctx)
	default:
		panic(fmt.Sprintf("Unknown contract operation: %s", input.Op))
	}
	return result, err
}

func main() {
	var client *partly.Client
	defer func() {
		if client != nil {
			if err := client.Close(context.Background()); err != nil {
				panic(err)
			}
		}
	}()
	scanner := bufio.NewScanner(os.Stdin)
	output := json.NewEncoder(os.Stdout)
	for scanner.Scan() {
		var input command
		decoder := json.NewDecoder(bytes.NewReader(scanner.Bytes()))
		decoder.UseNumber()
		if err := decoder.Decode(&input); err != nil {
			panic(err)
		}
		result, err := execute(&client, input)
		if err != nil {
			result = map[string]any{"error": errorCode(err)}
			if errors.Is(err, partly.ErrContextType) {
				var detail *partly.Error
				if !errors.As(err, &detail) {
					panic(err)
				}
				var op any
				if detail.Op != "" {
					op = detail.Op
				}
				result = map[string]any{
					"error": "context_type", "flag_key": detail.FlagKey, "k": detail.K, "op": op,
					"expected_type": detail.ExpectedType, "actual_type": detail.ActualType,
				}
			}
		}
		if err := output.Encode(result); err != nil {
			panic(err)
		}
	}
	if err := scanner.Err(); err != nil {
		panic(err)
	}
}
