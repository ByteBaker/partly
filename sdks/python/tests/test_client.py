import asyncio
import copy
import json
import shutil
import ssl
import threading
from pathlib import Path

import certifi
import httpx
import pytest

from partly import (
    Client,
    ClientClosedError,
    Decision,
    FlagNotFoundError,
    InvalidContextError,
    InvalidPolicyError,
    PolicyNotReadyError,
    RefreshError,
)

POLICY = {"schema_version": 1, "project": "default", "environment": "development", "revision": 1,
          "flags": {"checkout": {"mode": "percentage", "percentage": 100}}}


def mocked(handler, interval=60, timeout=1):
    client = Client.builder().url("http://partly.test").secret_key("test-key").poll_interval(interval).timeout(timeout).build()
    client._http = httpx.AsyncClient(transport=httpx.MockTransport(handler))
    return client


async def test_invalid_refresh_keeps_policy_and_etag():
    state = {"bad": False}
    tags = []
    async def handler(request):
        tags.append(request.headers.get("if-none-match"))
        if state["bad"]:
            return httpx.Response(200, headers={"etag": '"bad"'}, json={"schema_version": 999})
        return httpx.Response(200, headers={"etag": '"good"'}, json=POLICY)
    client = mocked(handler)
    try:
        await client.init()
        state["bad"] = True
        for _ in range(2):
            with pytest.raises(InvalidPolicyError):
                await client.refresh()
            assert client.evaluate("checkout", context={"id": "u"}) is Decision.ENABLED
        assert tags == [None, '"good"', '"good"']
        assert client._etag == '"good"'
        assert isinstance(client.last_refresh_error, InvalidPolicyError)
    finally:
        await client.aclose()


@pytest.mark.parametrize("case", ["empty", "missing_etag", "unsupported_schema", "percentage", "wrong_type", "extra_field", "too_large"])
async def test_bad_initial_policy_is_an_error(case):
    policy = copy.deepcopy(POLICY)
    headers = {"etag": '"one"'}
    if case == "unsupported_schema": policy["schema_version"] = 2
    if case == "percentage": policy["flags"]["checkout"]["percentage"] = 101
    if case == "wrong_type": policy["flags"]["checkout"]["percentage"] = "100"
    if case == "extra_field": policy["flags"]["checkout"]["traits"] = {}
    if case == "missing_etag": headers = {}
    def handler(request):
        if case == "empty": return httpx.Response(200, headers=headers, content=b"")
        if case == "too_large": return httpx.Response(200, headers=headers, content=b"x" * (1024 * 1024 + 1))
        return httpx.Response(200, headers=headers, json=policy)
    client = mocked(handler)
    try:
        with pytest.raises(InvalidPolicyError): await client.init()
        with pytest.raises(PolicyNotReadyError): client.evaluate("checkout", context={"id": "u"})
        assert client._poll_task is None
    finally:
        await client.aclose()


async def test_304_skips_parsing_and_initial_304_fails(monkeypatch):
    calls = 0
    def handler(request):
        nonlocal calls
        calls += 1
        return httpx.Response(200, headers={"etag": '"one"'}, json=POLICY) if calls == 1 else httpx.Response(304)
    client = mocked(handler)
    try:
        await client.init()
        snapshot = client._snapshot
        def forbidden_parse(body): raise AssertionError("304 must not parse a policy")
        monkeypatch.setattr("partly.client.Snapshot", forbidden_parse)
        assert await client.refresh() is False
        assert client._snapshot is snapshot
    finally:
        await client.aclose()
    cold = mocked(lambda r: httpx.Response(304))
    try:
        with pytest.raises(InvalidPolicyError): await cold.init()
    finally:
        await cold.aclose()


async def test_init_refresh_are_async_and_repeated_init_starts_one_poller():
    entered = asyncio.Event()
    release = asyncio.Event()
    calls = 0
    async def handler(request):
        nonlocal calls
        calls += 1
        entered.set()
        await release.wait()
        return httpx.Response(200, headers={"etag": '"one"'}, json=POLICY)
    client = mocked(handler)
    try:
        task = asyncio.create_task(client.init())
        await asyncio.wait_for(entered.wait(), 1)
        ticks = 0
        for _ in range(5):
            await asyncio.sleep(0.005)
            ticks += 1
        assert ticks == 5 and not task.done()
        release.set()
        await task
        poller = client._poll_task
        await asyncio.gather(client.init(), client.init())
        assert client._poll_task is poller and calls == 1
        entered.clear(); release.clear()
        refresh = asyncio.create_task(client.refresh())
        await asyncio.wait_for(entered.wait(), 1)
        assert client.evaluate("checkout", context={"id": "u"}) is Decision.ENABLED
        await asyncio.sleep(0.01)
        assert not refresh.done()
        release.set()
        assert await refresh
    finally:
        release.set()
        await client.aclose()
    assert poller.done()
    with pytest.raises(ClientClosedError): await client.init()
    with pytest.raises(ClientClosedError): await client.refresh()
    await client.aclose()


async def test_close_during_init_does_not_leave_a_poller():
    entered = asyncio.Event()
    release = asyncio.Event()
    async def handler(request):
        entered.set()
        await release.wait()
        return httpx.Response(200, headers={"etag": '"one"'}, json=POLICY)
    client = mocked(handler)
    task = asyncio.create_task(client.init())
    await entered.wait()
    close = asyncio.create_task(client.aclose())
    await asyncio.sleep(0)
    release.set()
    with pytest.raises(ClientClosedError): await task
    await close
    assert client._poll_task is None and client._http.is_closed
    assert client._snapshot is None and client._etag is None


@pytest.mark.parametrize("status", [200, 304])
async def test_close_during_refresh_keeps_last_accepted_policy(status):
    entered = asyncio.Event()
    release = asyncio.Event()
    calls = 0

    async def handler(request):
        nonlocal calls
        calls += 1
        if calls == 1:
            return httpx.Response(200, headers={"etag": '"good"'}, json=POLICY)
        entered.set()
        await release.wait()
        return httpx.Response(status, headers={"etag": '"good"'}, json={**POLICY, "flags": {}})

    client = mocked(handler)
    await client.init()
    snapshot = client._snapshot
    refresh = asyncio.create_task(client.refresh())
    await entered.wait()
    close = asyncio.create_task(client.aclose())
    await asyncio.sleep(0)
    release.set()
    try:
        with pytest.raises(ClientClosedError):
            await refresh
    finally:
        await close
    assert client._snapshot is snapshot and client._etag == '"good"'


async def test_policy_validation_does_not_block_the_event_loop(monkeypatch):
    from partly._native import Snapshot

    entered = asyncio.Event()
    release = threading.Event()
    loop = asyncio.get_running_loop()

    def delayed_parse(body):
        loop.call_soon_threadsafe(entered.set)
        release.wait(timeout=2)
        return Snapshot(body)

    monkeypatch.setattr("partly.client.Snapshot", delayed_parse)
    client = mocked(lambda request: httpx.Response(200, headers={"etag": '"good"'}, json=POLICY))
    init = asyncio.create_task(client.init())
    try:
        await asyncio.wait_for(entered.wait(), timeout=1)
        assert not init.done()
        close = asyncio.create_task(client.aclose())
        await asyncio.sleep(0)
        release.set()
        with pytest.raises(ClientClosedError):
            await init
        await close
        assert client._snapshot is None and client._etag is None
    finally:
        release.set()
        await asyncio.gather(init, return_exceptions=True)
        await client.aclose()


async def test_canceled_policy_validation_cannot_replace_snapshot(monkeypatch):
    from partly._native import Snapshot

    entered = asyncio.Event()
    release = threading.Event()
    loop = asyncio.get_running_loop()
    finished = asyncio.Event()
    client = mocked(lambda request: httpx.Response(200, headers={"etag": '"good"'}, json=POLICY))
    await client.init()
    snapshot = client._snapshot

    def delayed_parse(body):
        loop.call_soon_threadsafe(entered.set)
        release.wait(timeout=2)
        result = Snapshot(body)
        loop.call_soon_threadsafe(finished.set)
        return result

    monkeypatch.setattr("partly.client.Snapshot", delayed_parse)
    refresh = asyncio.create_task(client.refresh())
    try:
        await asyncio.wait_for(entered.wait(), timeout=1)
        refresh.cancel()
        with pytest.raises(asyncio.CancelledError):
            await refresh
        release.set()
        await asyncio.wait_for(finished.wait(), timeout=1)
        assert client._snapshot is snapshot and client._etag == '"good"'
    finally:
        release.set()
        await asyncio.gather(refresh, return_exceptions=True)
        await client.aclose()


@pytest.mark.parametrize("etag", [b'"bad\x00tag"', b'"bad\ntag"', b'"bad"tag"', b'"bad tag"', b'"\xe9"'])
async def test_invalid_etag_never_replaces_accepted_tag(etag):
    state = {"bad": False}
    def handler(request):
        tag = etag if state["bad"] else b'"good"'
        return httpx.Response(200, headers={b"etag": tag}, json=POLICY)
    client = mocked(handler)
    try:
        await client.init()
        snapshot = client._snapshot
        state["bad"] = True
        with pytest.raises(InvalidPolicyError):
            await client.refresh()
        assert client._snapshot is snapshot and client._etag == '"good"'
    finally:
        await client.aclose()


async def test_timeout_cancellation_retry_and_scope_validation():
    state = {"slow": True, "scope": False}
    async def handler(request):
        if state["slow"]: await asyncio.sleep(1)
        policy = {**POLICY, "environment": "production"} if state["scope"] else POLICY
        return httpx.Response(200, headers={"etag": '"one"'}, json=policy)
    client = mocked(handler, timeout=0.05)
    try:
        with pytest.raises(RefreshError): await client.init()
        assert client._snapshot is None
        task = asyncio.create_task(client.init())
        await asyncio.sleep(0.005)
        task.cancel()
        with pytest.raises(asyncio.CancelledError): await task
        state["slow"] = False
        await client.init()
        state["scope"] = True
        with pytest.raises(InvalidPolicyError): await client.refresh()
        assert client._snapshot.environment == "development"
        assert client.evaluate("checkout", context={"id": "u"}) is Decision.ENABLED
    finally:
        await client.aclose()


async def test_explicit_decisions_context_and_native_hash():
    policy = copy.deepcopy(POLICY)
    policy["flags"]["checkout"]["percentage"] = 94
    client = mocked(lambda r: httpx.Response(200, headers={"etag": '"one"'}, json=policy))
    try:
        await client.init()
        # Fixed Rust bucket for this pair is 9426, outside 94% and inside 95%.
        assert client.evaluate("checkout", context={"id": "user-42"}) is Decision.DISABLED
        policy["flags"]["checkout"]["percentage"] = 95
        await client.refresh()
        assert client.evaluate("checkout", context={"id": "user-42"}) is Decision.ENABLED
        for user in (None, 42, "", " ", "x" * 257):
            with pytest.raises(InvalidContextError): client.evaluate("checkout", context={"id": user})
        with pytest.raises(FlagNotFoundError): client.evaluate("absent", context={"id": "u"})
        with pytest.raises(TypeError): bool(Decision.DISABLED)
    finally:
        await client.aclose()


def test_builder_validation_and_no_network(monkeypatch):
    def forbidden(*args, **kwargs): raise AssertionError("build must not create HTTP resources")
    monkeypatch.setattr(httpx, "AsyncClient", forbidden)
    monkeypatch.setattr(ssl, "create_default_context", forbidden)
    builder = Client.builder().url("http://localhost:8080").secret_key("test")
    assert builder.build()._http is None
    assert builder.ca_bundle(Path("missing-ca.pem")).build()._ca_bundle == "missing-ca.pem"
    for value in (0, -1, float("inf"), float("nan"), True, 10**1000):
        with pytest.raises(ValueError): builder.poll_interval(value).build()
    for url in (None, 42, "", "ftp://localhost", "http://u:p@localhost", "http://@localhost",
                "http://localhost/path", "http://localhost:99999", "http://localhost?", "http://localhost#",
                "http://localhost\n"):
        with pytest.raises(ValueError): Client.builder().url(url).secret_key("test").build()
    for key in ("test\x00key", "test\x7fkey", "test\nkey", "test key"):
        with pytest.raises(ValueError): Client.builder().url("http://localhost").secret_key(key).build()
    for path in (42, False, b"ca.pem", "", "bad\0path"):
        with pytest.raises(ValueError): Client.builder().url("https://localhost").secret_key("test").ca_bundle(path).build()


@pytest.mark.parametrize("initial", ["missing", "invalid"])
async def test_ca_bundle_failure_is_safe_and_retryable(tmp_path, monkeypatch, initial):
    bundle = tmp_path / "private-key.pem"
    if initial == "invalid":
        await asyncio.to_thread(bundle.write_text, "not a certificate")
    original_client = httpx.AsyncClient
    created = []

    def create_client(**options):
        assert isinstance(options["verify"], ssl.SSLContext)
        assert options["verify"].check_hostname
        assert options["verify"].verify_mode == ssl.CERT_REQUIRED
        assert options["trust_env"] is False and options["follow_redirects"] is False
        client = original_client(**options, transport=httpx.MockTransport(
            lambda request: httpx.Response(200, headers={"etag": '"good"'}, json=POLICY)
        ))
        created.append(client)
        return client

    monkeypatch.setattr(httpx, "AsyncClient", create_client)
    client = Client.builder().url("https://partly.test").secret_key("private-key").ca_bundle(bundle).build()
    try:
        with pytest.raises(RefreshError, match="CA bundle") as caught:
            await client.init()
        assert "private-key" not in str(caught.value)
        assert client.last_refresh_error is caught.value
        assert not created and client._http is None and client._snapshot is None and client._poll_task is None
        await asyncio.to_thread(shutil.copyfile, certifi.where(), bundle)
        await client.init()
        assert client.last_refresh_error is None
        assert client.evaluate("checkout", context={"id": "u"}) is Decision.ENABLED
    finally:
        await client.aclose()
    assert len(created) == 1 and created[0].is_closed


@pytest.mark.parametrize("finish", ["close", "cancel", "success"])
async def test_tls_context_loading_is_async_and_cannot_create_client_after_close(monkeypatch, finish):
    entered = asyncio.Event()
    release = threading.Event()
    loop = asyncio.get_running_loop()
    main_thread = threading.get_ident()
    create_context = ssl.create_default_context
    original_client = httpx.AsyncClient
    created = []

    def load_context(*, cafile):
        assert threading.get_ident() != main_thread
        assert cafile == certifi.where()
        loop.call_soon_threadsafe(entered.set)
        release.wait(timeout=2)
        return create_context(cafile=cafile)

    def create_client(**options):
        assert threading.get_ident() == main_thread
        assert options["verify"].verify_mode == ssl.CERT_REQUIRED and options["verify"].check_hostname
        client = original_client(**options, transport=httpx.MockTransport(
            lambda request: httpx.Response(200, headers={"etag": '"good"'}, json=POLICY)
        ))
        created.append(client)
        return client

    monkeypatch.setattr(ssl, "create_default_context", load_context)
    monkeypatch.setattr(httpx, "AsyncClient", create_client)
    client = Client.builder().url("https://partly.test").secret_key("test-key").build()
    init = asyncio.create_task(client.init())
    try:
        await asyncio.wait_for(entered.wait(), 1)
        assert not init.done() and not created
        if finish == "cancel":
            init.cancel()
            with pytest.raises(asyncio.CancelledError):
                await init
            await client.aclose()
        elif finish == "close":
            close = asyncio.create_task(client.aclose())
            await asyncio.sleep(0)
            release.set()
            with pytest.raises(ClientClosedError):
                await init
            await close
        else:
            release.set()
            await init
            assert client.evaluate("checkout", context={"id": "u"}) is Decision.ENABLED
    finally:
        release.set()
        await asyncio.gather(init, return_exceptions=True)
        await client.aclose()
    assert len(created) == (1 if finish == "success" else 0)
    assert all(client.is_closed for client in created)


async def test_response_cleanup_failure_does_not_commit_snapshot():
    class BrokenStream(httpx.AsyncByteStream):
        async def __aiter__(self):
            yield json.dumps({**POLICY, "flags": {}}).encode()

        async def aclose(self):
            raise httpx.ReadError("Connection failed during response cleanup")

    state = {"broken": False}
    def handler(request):
        if state["broken"]:
            return httpx.Response(200, headers={"etag": '"broken"'}, stream=BrokenStream())
        return httpx.Response(200, headers={"etag": '"good"'}, json=POLICY)
    client = mocked(handler)
    try:
        await client.init()
        state["broken"] = True
        with pytest.raises(RefreshError): await client.refresh()
        assert client._etag == '"good"'
        assert client.evaluate("checkout", context={"id": "u"}) is Decision.ENABLED
    finally:
        await client.aclose()
