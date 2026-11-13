// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

package main

import (
	"context"
	"fmt"
	"os"

	partly "github.com/ByteBaker/partly/sdk/go"
	provider "github.com/ByteBaker/partly/sdk/go/openfeature"
	of "github.com/open-feature/go-sdk/openfeature"
)

const controlPlaneURL = "http://127.0.0.1:8080"
const environment = "development"
const applicationID = "YOUR_APPLICATION_ID"
const apiKey = "YOUR_API_KEY"

func checkout(ctx context.Context, flags *of.Client, evaluationContext of.EvaluationContext) (string, error) {
	result, err := flags.ObjectValueDetails(ctx, "checkout_settings", map[string]any{}, evaluationContext)
	if err != nil {
		return "", err
	}
	return fmt.Sprint(result.Value), nil
}

func run() error {
	client, err := partly.NewClientBuilder().URL(controlPlaneURL).Environment(environment).ApplicationID(applicationID).SecretKey(apiKey).Build()
	if err != nil {
		return err
	}
	ctx := context.Background()
	defer client.Close(ctx) // Application shutdown.
	// Application startup, before accepting requests.
	if err := client.Init(ctx); err != nil {
		return err
	}
	defer of.Shutdown()
	if err := of.SetNamedProviderAndWait("partly", provider.New(client)); err != nil {
		return err
	}
	flags := of.NewClient("partly")
	// Two example requests reuse the client; evaluation is local.
	for _, userID := range []string{"user-123", "user-456"} {
		message, err := checkout(ctx, flags, of.NewEvaluationContext(userID, nil))
		if err != nil {
			return err
		}
		fmt.Println(message)
	}
	return nil
}

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, "Partly:", err)
		os.Exit(1)
	}
}
