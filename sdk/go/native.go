// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Apache-2.0

package partly

/*
#cgo LDFLAGS: -lpartly_ffi
#cgo darwin,arm64 LDFLAGS: -L${SRCDIR}/native/darwin_arm64
#cgo linux,amd64 LDFLAGS: -L${SRCDIR}/native/linux_amd64
#cgo darwin LDFLAGS: -framework Security -framework CoreFoundation
#cgo linux LDFLAGS: -ldl -lpthread -lm
#include <stdint.h>
#include <stddef.h>
typedef struct Snapshot Snapshot;
Snapshot *partly_snapshot_new(const uint8_t *, size_t, char **);
int partly_snapshot_has_flag(const Snapshot *, const uint8_t *, size_t);
char *partly_snapshot_evaluate(const Snapshot *, const uint8_t *, size_t);
void partly_snapshot_free(Snapshot *);
void partly_string_free(char *);
*/
import "C"

import (
	"encoding/json"
	"reflect"
	"runtime"
	"strconv"
	"strings"
	"unicode/utf8"
	"unsafe"
)

type snapshot struct {
	ptr   *C.Snapshot
	scope string
}

func newSnapshot(body []byte) (*snapshot, error) {
	// Nonempty allocation also gives the C boundary a valid pointer for an empty response.
	data := append(body, 0)
	var result *C.char
	ptr := C.partly_snapshot_new((*C.uint8_t)(unsafe.Pointer(&data[0])), C.size_t(len(body)), &result)
	defer C.partly_string_free(result)
	if ptr == nil {
		return nil, &Error{Kind: ErrInvalidPolicy, Message: "Policy validation failed"}
	}
	value := &snapshot{ptr: ptr, scope: C.GoString(result)}
	runtime.SetFinalizer(value, func(s *snapshot) { C.partly_snapshot_free(s.ptr) })
	return value, nil
}

func (s *snapshot) hasFlag(flag string) bool {
	key := append([]byte(flag), 0)
	exists := C.partly_snapshot_has_flag(s.ptr, (*C.uint8_t)(unsafe.Pointer(&key[0])), C.size_t(len(flag)))
	runtime.KeepAlive(s)
	return exists != 0
}

func (s *snapshot) resolve(flag string, context map[string]any, kind, expectedType string) (Resolution, error) {
	if !s.hasFlag(flag) {
		return Resolution{}, &Error{Kind: ErrFlagNotFound, Message: "Flag is not defined in the loaded policy", FlagKey: flag}
	}
	values := make(map[string]any, len(context))
	for k, v := range context {
		if !utf8.ValidString(k) {
			return Resolution{}, &Error{Kind: ErrInvalidContext, Message: "Context keys must contain valid UTF-8", FlagKey: flag}
		}
		switch value := v.(type) {
		case nil, bool, int, int8, int16, int32, int64, uint, uint8, uint16, uint32, uint64:
		case string:
			if !utf8.ValidString(value) {
				return Resolution{}, &Error{Kind: ErrInvalidContext, Message: "Context text must contain valid UTF-8", FlagKey: flag, K: k}
			}
		case float32:
			v = json.Number(strconv.FormatFloat(float64(value), 'e', -1, 64))
		case float64:
			// An exponent keeps Rust from reading a rounded decimal spelling as an exact integer.
			v = json.Number(strconv.FormatFloat(value, 'e', -1, 64))
		case json.Number:
			if value == "" {
				return Resolution{}, &Error{Kind: ErrInvalidContext, Message: "Context contains an invalid number", FlagKey: flag, K: k}
			}
		default:
			// Preserve only the type; Rust supplies rule details without calling custom marshalers.
			switch reflect.TypeOf(v).Kind() {
			case reflect.Array, reflect.Slice:
				v = []any{}
			default:
				v = map[string]any{}
			}
		}
		values[k] = v
	}
	data, err := json.Marshal(map[string]any{"flag": flag, "context": values, "kind": kind, "expected_type": expectedType})
	if err != nil {
		return Resolution{}, &Error{Kind: ErrInvalidContext, Message: "Context contains an invalid number"}
	}
	raw := C.partly_snapshot_evaluate(s.ptr, (*C.uint8_t)(unsafe.Pointer(&data[0])), C.size_t(len(data)))
	runtime.KeepAlive(s)
	defer C.partly_string_free(raw)
	var result struct {
		Resolution
		Code         string `json:"error"`
		Message      string `json:"message"`
		FlagKey      string `json:"flag_key"`
		K            string `json:"k"`
		Op           string `json:"op"`
		ExpectedType string `json:"expected_type"`
		ActualType   string `json:"actual_type"`
	}
	decoder := json.NewDecoder(strings.NewReader(C.GoString(raw)))
	decoder.UseNumber()
	if err := decoder.Decode(&result); err != nil {
		return Resolution{}, &Error{Kind: ErrInvalidContext, Message: "Invalid native response"}
	}
	if result.Code != "" {
		kind := ErrInvalidContext
		switch result.Code {
		case "result_type":
			kind = ErrResultType
		case "named_type":
			kind = ErrNamedType
		case "context_type":
			kind = ErrContextType
		case "flag_not_found":
			kind = ErrFlagNotFound
		}
		return Resolution{}, &Error{kind, result.Message, result.FlagKey, result.K, result.Op, result.ExpectedType, result.ActualType}
	}
	result.Value = resultNumbers(result.Value)
	valid := false
	switch result.Kind {
	case "boolean":
		_, valid = result.Value.(bool)
	case "string":
		_, valid = result.Value.(string)
	case "integer":
		_, valid = result.Value.(int64)
	case "float":
		_, valid = result.Value.(float64)
	case "object":
		_, valid = result.Value.(map[string]any)
	}
	if !valid {
		return Resolution{}, &Error{Kind: ErrInvalidPolicy, Message: "Invalid native result"}
	}
	return result.Resolution, nil
}

func (s *snapshot) evaluate(flag string, context map[string]any) (Decision, error) {
	result, err := s.resolve(flag, context, "boolean", "")
	if err != nil {
		return "", err
	}
	if result.Value == true {
		return Enabled, nil
	}
	return Disabled, nil
}
