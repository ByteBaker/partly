# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Elastic-2.0

import asyncio
import os
import subprocess
import threading
from contextlib import closing
from datetime import UTC, datetime
from pathlib import Path

import pytest
from conftest import app_id, make_key, save_flag
from openfeature import api
from openfeature.evaluation_context import EvaluationContext
from openfeature.event import ProviderEvent
from openfeature.exception import ErrorCode, ProviderFatalError, ProviderNotReadyError
from openfeature.hook import Hook
from test_sdk_contract import DRIVERS, ROOT, Driver, packaged_driver

from partly import Client, KeyRevokedError, RefreshError
from partly.openfeature import PartlyProvider


@pytest.mark.asyncio
async def test_openfeature_context_hooks_lifecycle_and_provider_isolation(server, admin):
    save_flag(admin, key="adult", flag={"mode": "custom", "action": "enable", "groups": [[{"k": "age", "op": "ge", "v": 18}]]})
    _, key = make_key(admin)

    def build():
        return (Client.builder().url(server.url).secret_key(key).environment("development")
                .application_id(app_id(admin)).poll_interval(3600).build())

    native, other = build(), build()
    calls = []

    class AgeHook(Hook):
        def before(self, hook_context, hints):
            calls.append("before")
            assert hook_context.provider_metadata.name == "Partly"
            assert hook_context.evaluation_context.attributes["age"] == 17
            return EvaluationContext(attributes={"age": 23})

        def after(self, hook_context, details, hints):
            calls.append("after")
            assert details.value is True

        def error(self, hook_context, exception, hints):
            calls.append("error")

        def finally_after(self, hook_context, details, hints):
            calls.append("finally")

    flags = api.get_client("lifecycle")
    failed, ready = threading.Event(), threading.Event()
    flags.add_handler(ProviderEvent.PROVIDER_ERROR, lambda _event: failed.set())
    flags.add_handler(ProviderEvent.PROVIDER_READY, lambda _event: ready.set())
    try:
        with pytest.raises(ProviderNotReadyError):
            api.set_provider_and_wait(PartlyProvider(native), "lifecycle")
        assert await asyncio.to_thread(failed.wait, 2)
        await native.init()
        provider = PartlyProvider(native)
        api.set_provider_and_wait(provider, "lifecycle")
        assert await asyncio.to_thread(ready.wait, 2)
        assert api.get_provider_metadata("lifecycle").name == "Partly"
        api.set_evaluation_context(EvaluationContext(attributes={"age": 17}))
        assert not flags.get_boolean_value("adult", True)
        flags.context = EvaluationContext(attributes={"age": 21})
        assert flags.get_boolean_value("adult", False)
        context = EvaluationContext(attributes={"age": 17, "date": datetime.now(UTC), "extra": [1, 2]})
        assert not flags.get_boolean_value("adult", True, context)
        flags.add_hooks([AgeHook()])
        assert flags.get_boolean_value("adult", False, context)
        assert calls == ["before", "after", "finally"]
        calls.clear()
        assert flags.get_boolean_details("missing", False, context).error_code == ErrorCode.FLAG_NOT_FOUND
        assert calls == ["before", "error", "finally"]

        save_flag(admin, key="adult", flag={"mode": "disabled"})
        await other.init()
        api.set_provider_and_wait(PartlyProvider(other), "isolated")
        assert not api.get_client("isolated").get_boolean_value("adult", True)
        assert flags.get_boolean_value("adult", False, context)
        replacement = PartlyProvider(other)
        api.set_provider_and_wait(replacement, "lifecycle")
        # A fresh client for the same domain observes replacement without stale state or hooks.
        assert not api.get_client("lifecycle").get_boolean_value("adult", True)
        api.shutdown()
        with pytest.raises(ProviderNotReadyError):
            replacement.resolve_boolean_details("adult", True)
        assert other.has_flag("adult")  # Providers borrow the native client.
    finally:
        api.shutdown()
        await native.aclose()
        await other.aclose()

@pytest.fixture(params=["rust", "go", "node"])
def of_driver(request, tmp_path):
    language = request.param
    packages = os.environ.get("PARTLY_SDK_PACKAGES")
    if packages:
        command = packaged_driver(language, tmp_path, Path(packages).resolve(), openfeature=True)
    elif language == "rust":
        subprocess.run(["cargo", "build", "--locked", "--offline", "-p", "partly-rust", "--features", "openfeature",
                        "--example", "openfeature-contract"], cwd=ROOT, check=True, timeout=180)
        command = [str(ROOT / "target/debug/examples/openfeature-contract")]
    elif language == "go":
        subprocess.run(["sh", str(ROOT / "sdk/go/build.sh")], cwd=ROOT, check=True, timeout=120)
        command = [str(tmp_path / "go-openfeature")]
        subprocess.run(["go", "build", "-a", "-o", command[0], "."], cwd=DRIVERS / "go_openfeature", check=True, timeout=300)
    else:
        subprocess.run(["node", str(ROOT / "sdk/node/build.js")], cwd=ROOT, check=True, timeout=120)
        command = ["node", str(DRIVERS / "node/openfeature.mjs")]
    instance = Driver(command, tmp_path, cwd=tmp_path if packages else ROOT)
    try:
        yield language, instance
        instance.finish()
    finally:
        instance.stop()


def test_openfeature_cross_language_server_contract(of_driver, server, admin):
    language, driver = of_driver
    save_flag(admin, key="off", flag={"mode": "disabled"})
    save_flag(admin, key="rollout", percentage=50)
    save_flag(admin, key="adult", flag={"mode": "custom", "action": "enable", "groups": [[{"k": "age", "op": "ge", "v": 18}]]})
    key_id, key = make_key(admin)
    assert driver.call("build", url=server.url, key=key) == {"ok": True}
    assert driver.call("init") == {"ok": True}

    def evaluate(flag, context=None, fallback=False):
        return driver.call("evaluate", flag=flag, context=context or {}, fallback=fallback)

    off = evaluate("off", fallback=True)
    assert off == {"value": False, "error": "", "variant": "disabled", "reason": "CACHED"}
    assert evaluate("adult", {"age": 21, "extra": {"unused": True}})["value"] is True
    for context in [{}, {"age": "private"}, {"age": True}, {"age": {"nested": 21}}]:
        result = evaluate("adult", context, fallback=True)
        assert result["error"] == "INVALID_CONTEXT"
        if language != "rust":
            assert result["value"] is True
    missing = evaluate("missing", fallback=True)
    assert missing["error"] == "FLAG_NOT_FOUND"
    if language != "rust":
        assert missing["value"] is True
    selected = evaluate("rollout", {"targetingKey": "user-42", "id": "ignored"})
    assert selected == evaluate("rollout", {"id": "user-42"})
    assert driver.call("refresh") == {"updated": False}
    save_flag(admin, key="off", flag={"mode": "enabled"})
    assert driver.call("refresh") == {"updated": True}
    assert evaluate("off")["value"] is True

    server.stop()
    assert driver.call("refresh")["error"]
    assert evaluate("off")["value"] is True
    server.start()
    # Sessions are in memory, so sign in again after restarting the server.
    with closing(server.admin(setup=False)) as current_admin:
        assert current_admin.delete(f"/api/keys/{key_id}").status_code == 204
    assert driver.call("refresh")["error"]
    revoked = evaluate("off")
    assert revoked["error"] == "PROVIDER_FATAL"
    if language != "rust":
        assert revoked["value"] is False
    assert driver.call("close") == {"ok": True}


@pytest.mark.asyncio
async def test_openfeature_real_server_lifecycle_context_defaults_and_revocation(server, admin):
    save_flag(admin, key="off", flag={"mode": "disabled"})
    save_flag(admin, key="rollout", percentage=50)
    save_flag(admin, key="adult", flag={"mode": "custom", "action": "enable", "groups": [[{"k": "age", "op": "ge", "v": 18}]]})
    key_id, key = make_key(admin)
    native = (Client.builder().url(server.url).secret_key(key).environment("development")
              .application_id(app_id(admin)).poll_interval(3600).build())
    provider = PartlyProvider(native)
    try:
        with pytest.raises(ProviderNotReadyError):
            provider.initialize(EvaluationContext())
        await native.init()
        api.set_provider_and_wait(provider, "partly-test")
        flags = api.get_client("partly-test")
        details = flags.get_boolean_details("off", True)
        assert details.value is False and details.error_code is None
        assert details.reason == "CACHED" and details.variant == "disabled"
        assert flags.get_boolean_details("missing", True).error_code == ErrorCode.FLAG_NOT_FOUND
        assert flags.get_boolean_value("missing", True) is True
        for method, fallback in [(flags.get_string_details, "fallback"), (flags.get_integer_details, 7),
                                 (flags.get_float_details, 2.5), (flags.get_object_details, {"safe": True})]:
            result = method("off", fallback)
            assert result.value == fallback and result.error_code == ErrorCode.TYPE_MISMATCH
            assert method("missing", fallback).error_code == ErrorCode.FLAG_NOT_FOUND
        assert flags.get_boolean_details("adult", False).error_code == ErrorCode.INVALID_CONTEXT
        assert flags.get_boolean_details("adult", False, EvaluationContext(attributes={"age": "secret"})).error_code == ErrorCode.INVALID_CONTEXT
        api.set_evaluation_context(EvaluationContext(attributes={"age": 21}))
        assert flags.get_boolean_value("adult", False)
        assert not flags.get_boolean_value("adult", True, EvaluationContext(attributes={"age": 17}))
        context = EvaluationContext(targeting_key="user-42", attributes={"id": "ignored", "extra": {"nested": True}})
        assert await flags.get_boolean_value_async("rollout", False, context) == (native.evaluate("rollout", context={"id": "user-42"}).value == "enabled")
        assert await native.refresh() is False
        save_flag(admin, key="off", flag={"mode": "enabled"})
        assert await native.refresh() is True
        assert flags.get_boolean_value("off", False)
        server.stop()
        with pytest.raises(RefreshError):
            await native.refresh()
        assert flags.get_boolean_value("off", False)
        server.start()
        with closing(server.admin(setup=False)) as current_admin:
            assert current_admin.delete(f"/api/keys/{key_id}").status_code == 204
        with pytest.raises(KeyRevokedError):
            await native.refresh()
        result = flags.get_boolean_details("off", False)
        assert result.value is False and result.error_code == ErrorCode.PROVIDER_FATAL
        with pytest.raises(ProviderFatalError):
            provider.initialize(EvaluationContext())
        with pytest.raises(ProviderNotReadyError):
            provider.resolve_boolean_details("off", False)
        provider.shutdown()
        with pytest.raises(ProviderNotReadyError):
            provider.resolve_boolean_details("off", False)
    finally:
        api.shutdown()
        await native.aclose()
