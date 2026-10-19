import asyncio
import os
import runpy
import sys

import httpx
import pytest
from conftest import ROOT, make_key, save_flag

from partly import (
    AuthenticationError,
    Client,
    Decision,
    FlagNotFoundError,
    InvalidContextError,
    PolicyNotReadyError,
    RefreshError,
)


def client_for(server, key, interval=60):
    return Client.builder().url(server.url).secret_key(key).poll_interval(interval).timeout(1).build()


async def wait_until(predicate):
    while not predicate():
        await asyncio.sleep(0.02)


async def test_real_sdk_rollout_refresh_and_outage(server, admin):
    key_id, key = make_key(admin)
    save_flag(admin, percentage=25)
    client = client_for(server, key)
    try:
        with pytest.raises(PolicyNotReadyError):
            client.evaluate("new_checkout", context={"id": "u"})
        await client.init()
        await client.init()
        assert await client.refresh() is False
        with pytest.raises(FlagNotFoundError, match="undefined"):
            client.evaluate("undefined", context={"id": "u"})
        with pytest.raises(InvalidContextError):
            client.evaluate("new_checkout")
        users = [f"user-{i}" for i in range(1000)]
        selected = {u for u in users if client.evaluate("new_checkout", context={"id": u}) is Decision.ENABLED}
        assert 180 < len(selected) < 320
        assert selected == {u for u in users if client.evaluate("new_checkout", context={"id": u}) is Decision.ENABLED}
        save_flag(admin, percentage=50)
        assert await client.refresh() is True
        expanded = {u for u in users if client.evaluate("new_checkout", context={"id": u}) is Decision.ENABLED}
        assert selected < expanded
        for percentage, enabled, expected in [(0, True, Decision.DISABLED), (100, True, Decision.ENABLED), (100, False, Decision.DISABLED)]:
            save_flag(admin, percentage=percentage, enabled=enabled)
            assert await client.refresh()
            assert {client.evaluate("new_checkout", context={"id": u}) for u in users} == {expected}
        save_flag(admin)
        await client.refresh()
        admin.delete(f"/api/keys/{key_id}")
        with pytest.raises(AuthenticationError):
            await client.refresh()
        assert client.evaluate("new_checkout", context={"id": "u"}) is Decision.ENABLED
        server.stop()
        with pytest.raises(RefreshError):
            await client.refresh()
        assert client.evaluate("new_checkout", context={"id": "u"}) is Decision.ENABLED
        cold = client_for(server, key)
        try:
            with pytest.raises(RefreshError):
                await cold.init()
            with pytest.raises(PolicyNotReadyError):
                cold.evaluate("new_checkout", context={"id": "u"})
        finally:
            await cold.aclose()
    finally:
        await client.aclose()
    assert client._poll_task.done()


async def test_polling_and_example(server, admin):
    _, key = make_key(admin)
    save_flag(admin)
    client = client_for(server, key, interval=0.05)
    try:
        await client.init()
        save_flag(admin, enabled=False)
        await asyncio.wait_for(wait_until(
            lambda: client.evaluate("new_checkout", context={"id": "u"}) is Decision.DISABLED,
        ), timeout=3)
        save_flag(admin, percentage=50)
        process = await asyncio.create_subprocess_exec(
            sys.executable, str(ROOT / "examples/python/rollout.py"),
            env={**os.environ, "PARTLY_URL": server.url, "PARTLY_SDK_KEY": key},
            stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
        )
        stdout, stderr = await process.communicate()
        assert process.returncode == 0, stderr.decode()
        lines = stdout.decode().splitlines()
        await client.refresh()
        enabled = sum(
            client.evaluate("new_checkout", context={"id": f"user-{i}"}) is Decision.ENABLED
            for i in range(1, 10_001)
        )
        assert 4_800 <= enabled <= 5_200
        assert lines == [
            f"Enabled for {enabled:,} of 10,000 users ({enabled / 10_000:.1%}).",
            "Policy updated: False",
        ]
        server.stop()
        await asyncio.wait_for(wait_until(lambda: client.last_refresh_error is not None), timeout=3)
        assert client.evaluate("new_checkout", context={"id": "u"}) in (Decision.ENABLED, Decision.DISABLED)
    finally:
        await client.aclose()


async def test_rejected_init_and_empty_valid_policy(server, admin):
    bad = client_for(server, "invalid")
    try:
        with pytest.raises(AuthenticationError):
            await bad.init()
    finally:
        await bad.aclose()
    _, key = make_key(admin)
    empty = client_for(server, key)
    try:
        await empty.init()
        assert await empty.refresh() is False
        with pytest.raises(FlagNotFoundError):
            empty.evaluate("none", context={"id": "u"})
    finally:
        await empty.aclose()


@pytest.mark.parametrize("scenario, expected", [
    ("enabled", "Use the new checkout"),
    ("disabled", "Use the current checkout"),
    ("unknown_flag", "is not defined in the loaded policy"),
    ("missing_context", "required key is missing"),
    ("wrong_context_type", "expected number, got string"),
    ("invalid_key", "SDK key rejected"),
    ("empty_key", "Supply a non-empty ASCII SDK secret key"),
    ("invalid_url", "Supply a control-plane URL"),
    ("offline", "Policy request failed"),
])
async def test_dashboard_python_example(server, admin, tmp_path, scenario, expected):
    _, key = make_key(admin)
    if scenario != "unknown_flag":
        save_flag(admin, enabled=scenario != "disabled")
    if scenario in ("wrong_context_type", "missing_context"):
        field = "id" if scenario == "wrong_context_type" else "age"
        save_flag(admin, flag={"mode": "custom", "action": "enable", "groups": [[{"k": field, "op": "ge", "v": 1}]]})
    response = httpx.get(f"{server.url}/docs/python.py", trust_env=False)
    assert response.status_code == 200
    assert response.text == (ROOT / "examples/python/quickstart.py").read_text()
    api_key = {"invalid_key": "invalid", "empty_key": ""}.get(scenario, key)
    url = "invalid" if scenario == "invalid_url" else server.url
    script = tmp_path / "quickstart.py"
    script.write_text(response.text.replace(
        'CONTROL_PLANE_URL = "http://127.0.0.1:8080"', f"CONTROL_PLANE_URL = {url!r}",
    ).replace('API_KEY = "YOUR_API_KEY"', f"API_KEY = {api_key!r}"))
    if scenario == "offline":
        server.stop()
    process = await asyncio.create_subprocess_exec(
        sys.executable, str(script),
        stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
    )
    stdout, stderr = await process.communicate()
    assert key not in (stdout + stderr).decode()
    if scenario in ("enabled", "disabled"):
        assert process.returncode == 0, stderr.decode()
        assert stdout.decode().splitlines() == [expected]
    else:
        assert process.returncode != 0
        assert expected in stderr.decode()
        assert "Use the" not in stdout.decode()


async def test_quickstart_evaluates_locally_and_closes(server, admin, monkeypatch, capsys):
    _, key = make_key(admin)
    save_flag(admin)
    original_init = Client.init
    clients = []

    async def init_then_stop_server(client):
        await original_init(client)
        clients.append(client)
        server.stop()

    monkeypatch.setattr(Client, "init", init_then_stop_server)
    example = runpy.run_path(str(ROOT / "examples/python/quickstart.py"))
    example["main"].__globals__.update(CONTROL_PLANE_URL=server.url, API_KEY=key)
    await example["main"]()
    captured = capsys.readouterr()
    assert captured.err == ""
    assert "Use the new checkout" in captured.out
    assert clients[0]._http.is_closed
    assert clients[0]._poll_task.done()
