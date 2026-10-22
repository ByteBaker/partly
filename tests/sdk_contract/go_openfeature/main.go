// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

package main

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"os"
	"time"

	of "github.com/open-feature/go-sdk/openfeature"
	partly "partly.sh/sdk/go"
	provider "partly.sh/sdk/go/openfeature"
)

func run() error {
	ctx := context.Background()
	var native *partly.Client
	defer func() {
		of.Shutdown()
		if native != nil {
			_ = native.Close(ctx)
		}
	}()
	scanner := bufio.NewScanner(os.Stdin)
	output := json.NewEncoder(os.Stdout)
	for scanner.Scan() {
		var command struct {
			Op, URL, Key, Environment, Flag string
			ApplicationID                   string `json:"application_id"`
			Context                         map[string]any
			Fallback                        bool
		}
		if err := json.Unmarshal(scanner.Bytes(), &command); err != nil {
			return err
		}
		result := map[string]any{"ok": true}
		var err error
		switch command.Op {
		case "build":
			native, err = partly.NewClientBuilder().URL(command.URL).Environment(command.Environment).
				ApplicationID(command.ApplicationID).SecretKey(command.Key).PollInterval(time.Hour).Build()
		case "init":
			err = native.Init(ctx)
			if err == nil {
				err = of.SetNamedProviderAndWait("contract", provider.New(native))
			}
		case "evaluate":
			key, _ := command.Context[of.TargetingKey].(string)
			value, _ := of.NewClient("contract").BooleanValueDetails(ctx, command.Flag, command.Fallback, of.NewEvaluationContext(key, command.Context))
			result = map[string]any{"value": value.Value, "error": string(value.ErrorCode), "variant": value.Variant, "reason": string(value.Reason)}
		case "refresh":
			result = map[string]any{}
			result["updated"], err = native.Refresh(ctx)
		case "close":
			of.Shutdown()
			err = native.Close(ctx)
		default:
			return fmt.Errorf("unknown operation: %s", command.Op)
		}
		if err != nil {
			result = map[string]any{"error": err.Error()}
		}
		if err := output.Encode(result); err != nil {
			return err
		}
	}
	return scanner.Err()
}

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
