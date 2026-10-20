// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

package main

import (
	"context"
	"fmt"
	"os"

	partly "partly.sh/sdk/go"
)

const controlPlaneURL = "http://127.0.0.1:8080"
const apiKey = "YOUR_API_KEY"

func checkout(client *partly.Client, context map[string]any) (string, error) {
	decision, err := client.Evaluate("new_checkout", context)
	if err != nil {
		return "", err
	}
	if decision == partly.Enabled {
		return "Use the new checkout", nil
	}
	return "Use the current checkout", nil
}

func run() error {
	client, err := partly.NewClientBuilder().URL(controlPlaneURL).SecretKey(apiKey).Build()
	if err != nil {
		return err
	}
	ctx := context.Background()
	defer client.Close(ctx) // Application shutdown.
	// Application startup, before accepting requests.
	if err := client.Init(ctx); err != nil {
		return err
	}
	// Two example requests reuse the initialized client and its policy.
	for _, userID := range []string{"user-123", "user-456"} {
		message, err := checkout(client, map[string]any{"id": userID})
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
