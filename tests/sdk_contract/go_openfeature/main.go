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
			Op, URL, Key, Environment, Flag, Kind string
			ApplicationID                         string `json:"application_id"`
			Context                               map[string]any
			Fallback                              any
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
			value, _ := of.NewClient("contract").BooleanValueDetails(ctx, command.Flag, command.Fallback == true, of.NewEvaluationContext(key, command.Context))
			result = map[string]any{"value": value.Value, "error": string(value.ErrorCode), "variant": value.Variant, "reason": string(value.Reason)}
		case "resolve":
			key, _ := command.Context[of.TargetingKey].(string)
			client := of.NewClient("contract")
			context := of.NewEvaluationContext(key, command.Context)
			var value any
			var detail of.EvaluationDetails
			switch command.Kind {
			case "boolean":
				v, _ := client.BooleanValueDetails(ctx, command.Flag, command.Fallback == true, context)
				value, detail = v.Value, v.EvaluationDetails
			case "string":
				v, _ := client.StringValueDetails(ctx, command.Flag, command.Fallback.(string), context)
				value, detail = v.Value, v.EvaluationDetails
			case "integer":
				v, _ := client.IntValueDetails(ctx, command.Flag, int64(command.Fallback.(float64)), context)
				value, detail = v.Value, v.EvaluationDetails
			case "float":
				v, _ := client.FloatValueDetails(ctx, command.Flag, command.Fallback.(float64), context)
				value, detail = v.Value, v.EvaluationDetails
			case "object":
				v, _ := client.ObjectValueDetails(ctx, command.Flag, command.Fallback, context)
				value, detail = v.Value, v.EvaluationDetails
			}
			result = map[string]any{"value": value, "error": string(detail.ErrorCode), "variant": detail.Variant, "named_type": detail.FlagMetadata["partly.result_type"]}
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
