# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Elastic-2.0

"""Exercise the official Python OpenFeature API from the Rust E2E harness."""

import asyncio
import json
import sys
import threading
from contextlib import contextmanager
from datetime import UTC, datetime

from openfeature import api
from openfeature.evaluation_context import EvaluationContext
from openfeature.event import ProviderEvent
from openfeature.exception import ErrorCode, ProviderFatalError, ProviderNotReadyError
from openfeature.hook import Hook

from partly import Client, PartlyError
from partly.openfeature import PartlyProvider


@contextmanager
def raises(kind):
    try:
        yield
    except kind:
        pass
    else:
        raise AssertionError(f"Expected {kind.__name__}")


class AgeHook(Hook):
    def __init__(self):
        self.calls = []

    def before(self, hook_context, hints):
        self.calls.append("before")
        assert hook_context.provider_metadata.name == "Partly"
        assert hook_context.evaluation_context.attributes["age"] == 17
        return EvaluationContext(attributes={"age": 23})

    def after(self, hook_context, details, hints):
        self.calls.append("after")
        assert details.value is True

    def error(self, hook_context, exception, hints):
        self.calls.append("error")

    def finally_after(self, hook_context, details, hints):
        self.calls.append("finally")


def build(command):
    return (Client.builder().url(command["url"]).environment(command["environment"])
            .application_id(command["application_id"]).secret_key(command["key"])
            .poll_interval(3600).build())


async def main():
    native = other = provider = None
    configuration = {}
    context = EvaluationContext(attributes={"age": 17, "date": datetime.now(UTC), "extra": [1, 2]})
    try:
        while line := await asyncio.to_thread(sys.stdin.readline):
            command = json.loads(line)
            result = {"ok": True}
            try:
                op = command["op"]
                if op == "build":
                    configuration = command
                    native = build(command)
                elif op == "init":
                    provider = PartlyProvider(native)
                    with raises(ProviderNotReadyError):
                        provider.initialize(EvaluationContext())
                    await native.init()
                    api.set_provider_and_wait(provider, "contract")
                elif op == "evaluate":
                    attributes = command["context"].copy()
                    targeting_key = attributes.pop("targetingKey", None)
                    flags = api.get_client("contract")
                    details = await flags.get_boolean_details_async(
                        command["flag"], command.get("fallback", False),
                        EvaluationContext(targeting_key=targeting_key, attributes=attributes),
                    )
                    result = {"value": details.value, "error": details.error_code.value if details.error_code else "",
                              "variant": details.variant, "reason": details.reason}
                    if details.error_code == ErrorCode.PROVIDER_FATAL:
                        with raises(ProviderFatalError):
                            provider.initialize(EvaluationContext())
                        with raises(ProviderNotReadyError):
                            provider.resolve_boolean_details(command["flag"], False)
                elif op == "types_and_context":
                    flags = api.get_client("contract")
                    for method, fallback in [(flags.get_string_details, "fallback"), (flags.get_integer_details, 7),
                                             (flags.get_float_details, 2.5), (flags.get_object_details, {"safe": True})]:
                        details = method("off", fallback)
                        assert details.value == fallback and details.error_code == ErrorCode.TYPE_MISMATCH
                        assert method("missing", fallback).error_code == ErrorCode.FLAG_NOT_FOUND
                    api.set_evaluation_context(EvaluationContext(attributes={"age": 21}))
                    assert flags.get_boolean_value("adult", False)
                    assert not flags.get_boolean_value("adult", True, EvaluationContext(attributes={"age": 17}))
                    api.set_evaluation_context(EvaluationContext())
                elif op == "hooks_start":
                    flags = api.get_client("lifecycle")
                    failed, ready = threading.Event(), threading.Event()
                    flags.add_handler(ProviderEvent.PROVIDER_ERROR, lambda _event, failed=failed: failed.set())
                    flags.add_handler(ProviderEvent.PROVIDER_READY, lambda _event, ready=ready: ready.set())
                    with raises(ProviderNotReadyError):
                        api.set_provider_and_wait(PartlyProvider(native), "lifecycle")
                    assert await asyncio.to_thread(failed.wait, 2)
                    await native.init()
                    api.set_provider_and_wait(PartlyProvider(native), "lifecycle")
                    assert await asyncio.to_thread(ready.wait, 2)
                    assert api.get_provider_metadata("lifecycle").name == "Partly"
                    api.set_evaluation_context(EvaluationContext(attributes={"age": 17}))
                    assert not flags.get_boolean_value("adult", True)
                    flags.context = EvaluationContext(attributes={"age": 21})
                    assert flags.get_boolean_value("adult", False)
                    assert not flags.get_boolean_value("adult", True, context)
                    hook = AgeHook()
                    flags.add_hooks([hook])
                    assert flags.get_boolean_value("adult", False, context)
                    assert hook.calls == ["before", "after", "finally"]
                    hook.calls.clear()
                    assert flags.get_boolean_details("missing", False, context).error_code == ErrorCode.FLAG_NOT_FOUND
                    assert hook.calls == ["before", "error", "finally"]
                elif op == "hooks_finish":
                    other = build(configuration)
                    await other.init()
                    api.set_provider_and_wait(PartlyProvider(other), "isolated")
                    assert not api.get_client("isolated").get_boolean_value("adult", True)
                    assert flags.get_boolean_value("adult", False, context)
                    replacement = PartlyProvider(other)
                    api.set_provider_and_wait(replacement, "lifecycle")
                    assert not api.get_client("lifecycle").get_boolean_value("adult", True)
                    api.shutdown()
                    with raises(ProviderNotReadyError):
                        replacement.resolve_boolean_details("adult", True)
                    assert other.has_flag("adult")
                elif op == "refresh":
                    result = {"updated": await native.refresh()}
                elif op == "close":
                    api.shutdown()
                    if provider is not None:
                        with raises(ProviderNotReadyError):
                            provider.resolve_boolean_details("off", False)
                    await native.aclose()
                else:
                    raise ValueError(f"Unknown operation: {op}")
            except PartlyError as error:
                result = {"error": type(error).__name__}
            print(json.dumps(result), flush=True)
    finally:
        api.shutdown()
        for client in (native, other):
            if client is not None:
                await client.aclose()


asyncio.run(main())
