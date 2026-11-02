// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

package partly

import "encoding/json"

type Resolution struct {
	Value     any    `json:"value"`
	Kind      string `json:"kind"`
	Variant   string `json:"variant"`
	NamedType string `json:"named_type,omitempty"`
}

func resultNumbers(value any) any {
	switch v := value.(type) {
	case json.Number:
		if integer, err := v.Int64(); err == nil {
			return integer
		}
		number, _ := v.Float64()
		return number
	case map[string]any:
		for key, field := range v {
			v[key] = resultNumbers(field)
		}
	}
	return value
}

func (c *Client) EvaluateString(flag string, context map[string]any) (string, error) {
	result, err := c.Resolve(flag, context, "string", "")
	if err != nil {
		return "", err
	}
	value, ok := result.Value.(string)
	if !ok {
		return "", ErrResultType
	}
	return value, nil
}

func (c *Client) EvaluateInteger(flag string, context map[string]any) (int64, error) {
	result, err := c.Resolve(flag, context, "integer", "")
	if err != nil {
		return 0, err
	}
	value, ok := result.Value.(int64)
	if !ok {
		return 0, ErrResultType
	}
	return value, nil
}

func (c *Client) EvaluateFloat(flag string, context map[string]any) (float64, error) {
	result, err := c.Resolve(flag, context, "float", "")
	if err != nil {
		return 0, err
	}
	value, ok := result.Value.(float64)
	if !ok {
		return 0, ErrResultType
	}
	return value, nil
}

func (c *Client) EvaluateObject(flag string, context map[string]any) (map[string]any, error) {
	result, err := c.Resolve(flag, context, "object", "")
	if err != nil {
		return nil, err
	}
	value, ok := result.Value.(map[string]any)
	if !ok {
		return nil, ErrResultType
	}
	return value, nil
}

// EvaluateNamed is a Partly enhancement requiring the exact dashboard-defined type name.
func (c *Client) EvaluateNamed(flag string, context map[string]any, expectedType string) (any, error) {
	result, err := c.Resolve(flag, context, "named", expectedType)
	return result.Value, err
}
