# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Elastic-2.0

"""Exercise saved rules through the real server and installed native SDK."""

import pytest
from conftest import make_key

from partly import Client, ContextTypeError, Decision, InvalidContextError, RefreshError


def custom(groups, action="enable"):
    return {"mode": "custom", "action": action, "groups": groups}


def create_flag(admin, key, flag, environment="development"):
    response = admin.post(
        f"/api/environments/{environment}/flags", json={"key": key, **flag},
    )
    assert response.status_code == 201, response.text
    assert response.json() == {"key": key, **flag}


def client_for(server, secret):
    return Client.builder().url(server.url).secret_key(secret).poll_interval(3600).timeout(1).build()


async def test_all_typed_operators_through_saved_policy(server, admin):
    cases = [
        ("le", 18, 18, 19, True, "number", "boolean"),
        ("lt", 18, 17, 18, True, "number", "boolean"),
        ("ge", 18, 18, 17, True, "number", "boolean"),
        ("gt", 18, 19, 18, True, "number", "boolean"),
        ("eq", 18, 18.0, 19, "18", "number", "string"),
        ("prefix", "Pro", "Project", "project", 1, "string", "number"),
        ("suffix", "@company.com", "sam@company.com", "sam@COMPANY.COM", 1, "string", "number"),
        ("contains", "Beta", "Try Beta now", "Try beta now", 1, "string", "number"),
        ("matches", "pro", "pro", "professional", 1, "string", "number"),
        ("regex", "^user-[0-9]+$", "user-42", "User-42", 1, "string", "number"),
        ("is", True, True, False, 1, "boolean", "number"),
    ]
    for op, value, *_ in cases:
        create_flag(admin, f"rule_{op}", custom([[{"k": "value", "op": op, "v": value}]]))
    _, secret = make_key(admin)
    client = client_for(server, secret)
    try:
        await client.init()
        for op, _, matched, unmatched, wrong, expected_type, actual_type in cases:
            key = f"rule_{op}"
            assert client.evaluate(key, context={"value": matched}) is Decision.ENABLED
            assert client.evaluate(key, context={"value": unmatched}) is Decision.DISABLED
            with pytest.raises(ContextTypeError) as raised:
                client.evaluate(key, context={"value": wrong})
            error = raised.value
            assert isinstance(error, InvalidContextError)
            assert (error.flag_key, error.k, error.op) == (key, "value", op)
            assert (error.expected_type, error.actual_type) == (expected_type, actual_type)
            with pytest.raises(InvalidContextError):
                client.evaluate(key, context={})
        assert await client.refresh() is False
        assert client.evaluate("rule_matches", context={"value": "Pro"}) is Decision.DISABLED
        response = admin.put("/api/environments/development/flags/rule_is", json=custom([
            [{"k": "value", "op": "is", "v": False}],
        ]))
        assert response.status_code == 200
        assert await client.refresh() is True
        assert client.evaluate("rule_is", context={"value": False}) is Decision.ENABLED
        assert client.evaluate("rule_is", context={"value": True}) is Decision.DISABLED
    finally:
        await client.aclose()


async def test_groups_errors_refresh_and_offline_rules(server, admin):
    groups = [
        [{"k": "plan", "op": "matches", "v": "pro"}, {"k": "age", "op": "ge", "v": 18}],
        [{"k": "email", "op": "suffix", "v": "@company.com"}],
    ]
    flag = custom(groups)
    create_flag(admin, "checkout", flag)
    _, secret = make_key(admin)
    client = client_for(server, secret)
    contexts = [
        ({"plan": "pro", "age": 23, "email": "sam@example.com"}, Decision.ENABLED),
        ({"plan": "free", "age": 16, "email": "sam@company.com"}, Decision.ENABLED),
        ({"plan": "pro", "age": 16, "email": "sam@example.com"}, Decision.DISABLED),
    ]
    try:
        await client.init()
        for context, expected in contexts:
            assert client.evaluate("checkout", context=context) is expected
        # A matching OR group must not conceal missing or wrongly typed required fields.
        with pytest.raises(InvalidContextError):
            client.evaluate("checkout", context={"email": "sam@company.com"})
        with pytest.raises(ContextTypeError) as raised:
            client.evaluate("checkout", context={
                "plan": "pro", "age": "private-context-value", "email": "sam@company.com",
            })
        assert "private-context-value" not in str(raised.value)
        assert "sam@company.com" not in str(raised.value)
        changed = custom(groups, action="disable")
        response = admin.put("/api/environments/development/flags/checkout", json=changed)
        assert response.status_code == 200
        assert response.json() == {"key": "checkout", **changed}
        assert client.evaluate("checkout", context=contexts[0][0]) is Decision.ENABLED
        assert await client.refresh() is True
        for context, previous in contexts:
            expected = Decision.DISABLED if previous is Decision.ENABLED else Decision.ENABLED
            assert client.evaluate("checkout", context=context) is expected
        assert await client.refresh() is False
        server.stop()
        with pytest.raises(RefreshError):
            await client.refresh()
        assert client.evaluate("checkout", context=contexts[0][0]) is Decision.DISABLED
    finally:
        await client.aclose()


async def test_environment_policies_restart_and_population_consistency(server, admin):
    create_flag(admin, "checkout", {"mode": "percentage", "percentage": 50})
    create_flag(admin, "checkout", {"mode": "disabled"}, environment="production")
    create_flag(admin, "public", {"mode": "enabled"})
    _, development_secret = make_key(admin)
    _, production_secret = make_key(admin, environment="production")
    development = client_for(server, development_secret)
    production = client_for(server, production_secret)
    try:
        await development.init()
        await production.init()
        first_ten = {i: development.evaluate("checkout", context={"id": f"user-{i}"}) for i in range(10)}
        first_hundred = {i: development.evaluate("checkout", context={"id": f"user-{i}"}) for i in range(100)}
        assert first_ten == {i: first_hundred[i] for i in first_ten}
        assert set(first_hundred.values()) == {Decision.ENABLED, Decision.DISABLED}
        assert development.evaluate("public", context={}) is Decision.ENABLED
        assert production.evaluate("checkout", context={}) is Decision.DISABLED
        with pytest.raises(ContextTypeError):
            development.evaluate("checkout", context={"id": 123})
        server.stop()
        server.start()
        cold = client_for(server, development_secret)
        try:
            await cold.init()
            assert first_ten == {
                i: cold.evaluate("checkout", context={"id": f"user-{i}"}) for i in first_ten
            }
            assert await cold.refresh() is False
        finally:
            await cold.aclose()
    finally:
        await development.aclose()
        await production.aclose()


async def test_numeric_rules_keep_precision_across_api_storage_and_sdk(server, admin):
    limits = [("minimum", -(2**63), 1), ("maximum", 2**64 - 1, -1)]
    for key, value, _ in limits:
        create_flag(admin, key, custom([[{"k": "number", "op": "eq", "v": value}]]))
    _, secret = make_key(admin)
    client = client_for(server, secret)
    try:
        await client.init()
        for key, value, step in limits:
            assert client.evaluate(key, context={"number": value}) is Decision.ENABLED
            assert client.evaluate(key, context={"number": value + step}) is Decision.DISABLED
            # Out-of-range integer literals must not be rounded into valid rules.
            response = admin.put(
                f"/api/environments/development/flags/{key}",
                json=custom([[{"k": "number", "op": "eq", "v": value - step}]]),
            )
            assert response.status_code == 422, response.text
        assert await client.refresh() is False
        assert client.evaluate("minimum", context={"number": -(2**63)}) is Decision.ENABLED
    finally:
        await client.aclose()
