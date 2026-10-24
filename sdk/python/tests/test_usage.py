# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Apache-2.0

import asyncio
import json
import threading

import httpx
import pytest

from partly import (
    Client,
    Decision,
    FlagNotFoundError,
    InvalidContextError,
    KeyRevokedError,
    RefreshError,
)


@pytest.fixture
async def usage(monkeypatch):
    now = [0.0]
    monkeypatch.setattr("partly.client.monotonic", lambda: now[0])
    policy = {"schema_version": 1, "application_id": "app_test", "project": "default", "environment": "development", "revision": 1,
              "flags": {**{f"f{i}": {"mode": "enabled"} for i in range(300)}, "off": {"mode": "disabled"},
                        "context": {"mode": "percentage", "percentage": 50}}}
    requests = []
    state = {"status": 204, "gate": None, "refresh_gate": None}

    async def handler(request):
        if request.url.path == "/api/usage":
            requests.append(request)
            if state["gate"] is not None:
                await state["gate"].wait()
            return httpx.Response(state["status"], headers={"X-Partly-Error": "key_revoked"})
        if state["refresh_gate"] is not None:
            await state["refresh_gate"].wait()
        return httpx.Response(200, headers={"etag": '"one"'}, json=policy)

    client = Client.builder().url("http://partly.test").environment("development").application_id("app_test").secret_key("test-key").poll_interval(3600).timeout(1).build()
    client._http = httpx.AsyncClient(transport=httpx.MockTransport(handler))
    await client.init()
    try:
        yield client, now, requests, state, policy
    finally:
        for key in ("gate", "refresh_gate"):
            if state[key] is not None:
                state[key].set()
        await client.aclose()


async def finish_report(client):
    await asyncio.sleep(0)
    assert client._usage_task is not None
    await client._usage_task


async def test_usage_is_lazy_distinct_and_counts_disabled_and_invalid_context(usage):
    client, now, requests, _, _ = usage
    assert client.has_flag("f1")
    with pytest.raises(FlagNotFoundError):
        client.evaluate("unknown", context={})
    assert client.evaluate("off", context={}) is Decision.DISABLED
    with pytest.raises(InvalidContextError):
        client.evaluate("context", context={})
    for _ in range(200):
        client.evaluate("off", context={})
    now[0] = 59.9
    client.evaluate("f0", context={})
    await asyncio.sleep(0)
    assert requests == []
    now[0] = 60
    await asyncio.sleep(0)
    assert requests == []
    client.evaluate("off", context={})
    await finish_report(client)
    assert len(requests) == 1
    request = requests[0]
    assert request.method == "POST"
    assert json.loads(request.content) == {"flags": ["off", "context", "f0"]}
    assert request.headers["authorization"] == "Bearer test-key"
    assert request.headers["x-partly-environment"] == "development"
    assert request.headers["x-partly-application"] == "app_test"
    assert "if-none-match" not in request.headers


async def test_threshold_and_evaluation_from_worker_threads(usage):
    client, _, requests, _, _ = usage
    await asyncio.gather(*(asyncio.to_thread(client.evaluate, f"f{i}", context={}) for i in range(127)))
    assert requests == []
    await asyncio.to_thread(client.evaluate, "f127", context={})
    await finish_report(client)
    assert len(requests) == 1
    assert len(set(json.loads(requests[0].content)["flags"])) == 128


async def test_worker_refresh_cannot_record_a_previously_unknown_flag(usage, monkeypatch):
    client, _, _, _, policy = usage
    entered = asyncio.Event()
    release = threading.Event()
    loop = asyncio.get_running_loop()
    record = client._record_usage

    def delayed_record(flag, snapshot):
        loop.call_soon_threadsafe(entered.set)
        assert release.wait(timeout=1)
        record(flag, snapshot)

    monkeypatch.setattr(client, "_record_usage", delayed_record)
    evaluation = asyncio.create_task(asyncio.to_thread(client.evaluate, "new", context={}))
    try:
        await asyncio.wait_for(entered.wait(), 1)
        policy["flags"] = {"new": {"mode": "enabled"}}
        await client.refresh()
    finally:
        release.set()
    with pytest.raises(FlagNotFoundError):
        await evaluation
    assert not client._usage_pending


async def test_one_inflight_preserves_reevaluations_and_overdue_fifo_tail(usage):
    client, now, requests, state, _ = usage
    state["gate"] = asyncio.Event()
    now[0] = 60
    for i in range(200):
        client.evaluate(f"f{i}", context={})
    await asyncio.sleep(0)
    await asyncio.sleep(0)
    client.evaluate("f0", context={})
    assert len(requests) == 1
    assert json.loads(requests[0].content)["flags"] == [f"f{i}" for i in range(128)]
    state["gate"].set()
    await finish_report(client)
    assert len(requests) == 1
    client.evaluate("f0", context={})
    await finish_report(client)
    assert json.loads(requests[1].content)["flags"] == [*[f"f{i}" for i in range(128, 200)], "f0"]


@pytest.mark.parametrize("status", [400, 401, 500, 302])
async def test_failure_preserves_policy_and_refresh_error_with_full_queue_cooldown(usage, status):
    client, now, requests, state, _ = usage
    state["status"] = status
    # Only a confirmed revoked-key response is terminal.
    if status == 401:
        async def rejected(request):
            requests.append(request)
            return httpx.Response(401)
        await client._http.aclose()
        client._http = httpx.AsyncClient(transport=httpx.MockTransport(rejected))
    error = RefreshError("Earlier refresh failed")
    client.last_refresh_error = error
    for i in range(128):
        client.evaluate(f"f{i}", context={})
    await finish_report(client)
    assert client.last_refresh_error is error
    now[0] = 59.9
    assert client.evaluate("f0", context={}) is Decision.ENABLED
    await asyncio.sleep(0)
    assert len(requests) == 1
    now[0] = 60
    client.evaluate("f0", context={})
    await finish_report(client)
    assert len(requests) == 2


async def test_refresh_prunes_pending_and_failed_inflight_names(usage):
    client, now, requests, state, policy = usage
    state["gate"] = asyncio.Event()
    state["status"] = 500
    now[0] = 60
    client.evaluate("f0", context={})
    await asyncio.sleep(0)
    client.evaluate("f1", context={})
    client.evaluate("f2", context={})
    policy["flags"] = {"f1": {"mode": "enabled"}}
    await client.refresh()
    assert list(client._usage_pending) == ["f1"]
    state["gate"].set()
    await finish_report(client)
    assert list(client._usage_pending) == ["f1"]
    state["status"] = 204
    now[0] = 120
    client.evaluate("f1", context={})
    await finish_report(client)
    assert json.loads(requests[1].content) == {"flags": ["f1"]}


async def test_usage_revocation_prevents_concurrent_refresh_restoring_policy(usage):
    client, now, requests, state, _ = usage
    state["gate"] = asyncio.Event()
    state["refresh_gate"] = asyncio.Event()
    state["status"] = 401
    now[0] = 60
    client.evaluate("f0", context={})
    await asyncio.sleep(0)
    refresh = asyncio.create_task(client.refresh())
    await asyncio.sleep(0)
    state["gate"].set()
    await finish_report(client)
    state["refresh_gate"].set()
    with pytest.raises(KeyRevokedError):
        await refresh
    assert client._snapshot is None and client._etag is None and not client._usage_pending
    assert isinstance(client.last_refresh_error, KeyRevokedError)
    assert client._poll_task.done()
    with pytest.raises(KeyRevokedError):
        client.evaluate("f0", context={})
    with pytest.raises(KeyRevokedError):
        await client.init()
    assert len(requests) == 1


async def test_close_cancels_reporting_and_keeps_local_evaluation(usage):
    client, now, requests, state, _ = usage
    state["gate"] = asyncio.Event()
    now[0] = 60
    client.evaluate("f0", context={})
    await asyncio.sleep(0)
    await asyncio.sleep(0)
    await asyncio.wait_for(client.aclose(), 0.1)
    assert client._usage_task.done()
    assert client.evaluate("f1", context={}) is Decision.ENABLED
    assert not client._usage_pending
    await client.aclose()
    assert len(requests) == 1


async def test_refresh_revocation_cancels_inflight_usage(usage, monkeypatch):
    client, now, requests, state, _ = usage
    state["gate"] = asyncio.Event()
    now[0] = 60
    client.evaluate("f0", context={})
    await asyncio.sleep(0)
    await asyncio.sleep(0)

    async def revoked():
        raise KeyRevokedError("API Key revoked")

    monkeypatch.setattr(client, "_request", revoked)
    with pytest.raises(KeyRevokedError):
        await client.refresh()
    await asyncio.sleep(0)
    assert client._usage_task.done()
    assert not client._usage_pending and client._snapshot is None and client._etag is None
    assert len(requests) == 1


async def test_usage_timeout_is_best_effort(usage):
    client, now, requests, state, _ = usage
    state["gate"] = asyncio.Event()
    client._timeout = 0.01
    now[0] = 60
    client.evaluate("f0", context={})
    await finish_report(client)
    assert list(client._usage_pending) == ["f0"]
    assert client.last_refresh_error is None
    assert client.evaluate("f0", context={}) is Decision.ENABLED
    assert len(requests) == 1
