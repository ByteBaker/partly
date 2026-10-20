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

func run() error {
	client, err := partly.NewClientBuilder().URL(controlPlaneURL).SecretKey(apiKey).Build()
	if err != nil {
		return err
	}
	ctx := context.Background()
	defer client.Close(ctx)
	if err := client.Init(ctx); err != nil {
		return err
	}
	decision, err := client.Evaluate("new_checkout", map[string]any{"id": "user-123"})
	if err != nil {
		return err
	}
	if decision == partly.Enabled {
		fmt.Println("Use the new checkout")
	} else {
		fmt.Println("Use the current checkout")
	}
	return nil
}

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, "Partly:", err)
		os.Exit(1)
	}
}
