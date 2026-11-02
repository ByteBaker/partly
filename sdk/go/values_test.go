// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

package partly

import (
	"errors"
	"testing"
)

func TestTypedGettersPreserveNumericKindsAndOwnedRecords(t *testing.T) {
	snapshot, err := newSnapshot([]byte(`{"schema_version":2,"application_id":"app_test","project":"default","environment":"development","revision":1,"types":{"Settings":{"kind":"record","fields":{"count":"integer","rate":"float"}}},"flags":{
 "record":{"mode":"typed","result_type":"Settings","selected":{"count":3,"rate":3},"other":{"count":0,"rate":0},"rollout":{"mode":"enabled"}},
 "integer":{"mode":"typed","result_type":"integer","selected":3,"other":0,"rollout":{"mode":"enabled"}},
 "float":{"mode":"typed","result_type":"float","selected":3,"other":0,"rollout":{"mode":"enabled"}},
 "string":{"mode":"typed","result_type":"string","selected":"yes","other":"no","rollout":{"mode":"enabled"}}
 }}`))
	if err != nil {
		t.Fatal(err)
	}
	client := &Client{policy: snapshot, closed: true}
	if value, err := client.EvaluateInteger("integer", nil); err != nil || value != 3 {
		t.Fatalf("%v %v", value, err)
	}
	if value, err := client.EvaluateFloat("float", nil); err != nil || value != 3 {
		t.Fatalf("%v %v", value, err)
	}
	if value, err := client.EvaluateString("string", nil); err != nil || value != "yes" {
		t.Fatalf("%v %v", value, err)
	}
	value, err := client.EvaluateObject("record", nil)
	if err != nil || value["count"] != int64(3) || value["rate"] != float64(3) {
		t.Fatalf("%#v %v", value, err)
	}
	value["count"] = int64(99)
	named, err := client.EvaluateNamed("record", nil, "Settings")
	if err != nil || named.(map[string]any)["count"] != int64(3) {
		t.Fatalf("Mutated cache: %#v %v", named, err)
	}
	_, err = client.EvaluateNamed("record", nil, "Other")
	var detail *Error
	if !errors.Is(err, ErrNamedType) || !errors.Is(err, ErrResultType) || !errors.As(err, &detail) || detail.ActualType != "Settings" || detail.ExpectedType != "Other" {
		t.Fatalf("%#v", err)
	}
	if _, err := client.EvaluateInteger("float", nil); !errors.Is(err, ErrResultType) {
		t.Fatal(err)
	}
}
