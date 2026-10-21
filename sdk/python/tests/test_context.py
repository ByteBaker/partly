# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Apache-2.0

"""Native boundary contracts that Python's permissive types can otherwise hide."""
import json

import pytest

from partly import (
    ContextTypeError,
    FlagNotFoundError,
    InvalidContextError,
    InvalidPolicyError,
)
from partly._native import Snapshot


def snapshot(flag):
    return Snapshot(json.dumps({"schema_version": 1, "application_id": "app_test", "project": "default", "environment": "development",
                                "revision": 1, "flags": {"checkout": flag}}).encode())


def rule(op, value):
    return snapshot({"mode": "custom", "action": "enable", "groups": [[{"k": "field", "op": op, "v": value}]]})


@pytest.mark.parametrize("op,value,yes,no", [
    ("le", 18, 18.0, 19), ("lt", 18, -0.5, 18),
    ("ge", 18, 18, 17.9), ("gt", 18, 18.1, 18), ("eq", 18, 18.0, 18.1),
    ("prefix", "Pro", "Product", "product"), ("suffix", ".com", "a.com", "a.COM"),
    ("contains", "Pro", "aProZ", "aproZ"), ("matches", "Pro", "Pro", "ProPlus"),
    ("regex", "^Pro[0-9]+$", "Pro42", "pro42"), ("is", False, False, True),
])
def test_native_operators(op, value, yes, no):
    policy = rule(op, value)
    assert policy.evaluate("checkout", {"field": yes}) is True
    assert policy.evaluate("checkout", {"field": no}) is False


@pytest.mark.parametrize("op,value,actual,expected_type,actual_type", [
    ("ge", 18, "private-value", "number", "string"),
    ("eq", 1, True, "number", "boolean"),
    ("is", True, 1, "boolean", "number"),
    ("matches", "a", False, "string", "boolean"),
    ("regex", "a", [], "string", "array"),
    ("ge", 1, {}, "number", "object"),
    ("ge", 1, object(), "number", "object"),
    ("is", False, None, "boolean", "null"),
])
def test_context_type_details_do_not_include_values(op, value, actual, expected_type, actual_type):
    with pytest.raises(ContextTypeError) as caught:
        rule(op, value).evaluate("checkout", {"field": actual})
    error = caught.value
    assert isinstance(error, InvalidContextError)
    assert (error.flag_key, error.k, error.op, error.expected_type, error.actual_type) == (
        "checkout", "field", op, expected_type, actual_type)
    assert "private-value" not in str(error)


def test_only_percentage_requires_id():
    for mode, decision in [("enabled", True), ("disabled", False)]:
        assert snapshot({"mode": mode}).evaluate("checkout", {}) is decision
    assert rule("ge", 18).evaluate("checkout", {"field": 20}) is True
    policy = snapshot({"mode": "percentage", "percentage": 100})
    with pytest.raises(InvalidContextError):
        policy.evaluate("checkout", {})
    with pytest.raises(ContextTypeError) as caught:
        policy.evaluate("checkout", {"id": 5})
    assert caught.value.k == "id" and caught.value.op is None
    assert caught.value.expected_type == "string"
    assert caught.value.actual_type == "number"


@pytest.mark.parametrize("context", [None, [], "value", {1: "value"}, {"unused": {}}, {"unused": []},
                                     {"unused": None}, {"unused": float("nan")}, {"unused": float("inf")},
                                     {"unused": 2**64}, {"unused": -(2**63)-1}])
def test_context_requires_flat_supported_values(context):
    with pytest.raises(InvalidContextError):
        snapshot({"mode": "enabled"}).evaluate("checkout", context)


def test_invalid_unicode_does_not_echo_context_value():
    with pytest.raises(InvalidContextError) as caught:
        rule("matches", "test").evaluate("checkout", {"field": "private\ud800"})
    assert "private" not in str(caught.value)


def test_numeric_comparisons_preserve_integer_precision():
    assert rule("eq", 18).evaluate("checkout", {"field": 18.0}) is True
    assert rule("eq", 2**53).evaluate("checkout", {"field": 2**53 + 1}) is False
    assert rule("gt", float(2**53)).evaluate("checkout", {"field": 2**53 + 1}) is True
    assert rule("lt", float(2**64)).evaluate("checkout", {"field": 2**64 - 1}) is True


@pytest.mark.parametrize("value", [-(2**63) - 1, 2**64])
def test_out_of_range_integer_rules_do_not_round_to_other_values(value):
    with pytest.raises(InvalidPolicyError):
        rule("eq", value)


@pytest.mark.parametrize("marker", ["$serde_json::private::Number", "$serde_json::private::RawValue"])
def test_internal_json_marker_remains_an_invalid_object(marker):
    with pytest.raises(InvalidPolicyError):
        rule("eq", {marker: "18"})
    assert rule("matches", marker).evaluate("checkout", {"field": marker}) is True


@pytest.mark.parametrize("op,value", [("eq", True), ("eq", "1"), ("is", 1), ("matches", 1),
                                      ("bool", True), ("regex", "(?i)x"), ("regex", "(?i:x)"),
                                      ("regex", r"(a)\1"), ("regex", "(?=x)")])
def test_invalid_rules_rejected_before_evaluation(op, value):
    with pytest.raises(InvalidPolicyError):
        rule(op, value)


def test_unknown_flag_is_not_masked_by_invalid_context():
    with pytest.raises(FlagNotFoundError):
        snapshot({"mode": "enabled"}).evaluate("missing", None)


def test_rule_validation_does_not_short_circuit():
    policy = snapshot({"mode": "custom", "action": "enable", "groups": [
        [{"k": "staff", "op": "is", "v": True}],
        [{"k": "age", "op": "ge", "v": 18}],
    ]})
    with pytest.raises(InvalidContextError):
        policy.evaluate("checkout", {"staff": True})
    with pytest.raises(ContextTypeError):
        policy.evaluate("checkout", {"staff": True, "age": "18"})
