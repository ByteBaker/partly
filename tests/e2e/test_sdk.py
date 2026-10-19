import asyncio
import os
import runpy
import sys

import httpx
import pytest

from partly import (AuthenticationError, Client, Decision, FlagNotFoundError, InvalidContextError,
                    PolicyNotReadyError, RefreshError)
from conftest import ROOT, make_key, save_flag


def client_for(server, key, interval=60):
    return Client.builder().url(server.url).secret_key(key).poll_interval(interval).timeout(1).build()


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
        async with asyncio.timeout(3):
            while client.evaluate("new_checkout", context={"id": "u"}) is Decision.ENABLED:
                await asyncio.sleep(0.02)
        save_flag(admin, percentage=50)
        process = await asyncio.create_subprocess_exec(
            sys.executable, str(ROOT / "examples/python/rollout.py"),
            env={**os.environ, "PARTLY_URL": server.url, "PARTLY_SDK_KEY": key},
            stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
        )
        stdout, stderr = await process.communicate()
        assert process.returncode == 0, stderr.decode()
        lines = stdout.decode().splitlines()
        assert len(lines) == 10_001
        assert lines[-1] == "Policy updated: False"
        decisions = dict(line.split(": ", 1) for line in lines[:-1])
        assert set(decisions) == {f"user-{i}" for i in range(1, 10_001)}
        assert set(decisions.values()) == {"new checkout", "current checkout"}
        assert 4_800 <= sum(value == "new checkout" for value in decisions.values()) <= 5_200
        server.stop()
        async with asyncio.timeout(3):
            while client.last_refresh_error is None:
                await asyncio.sleep(0.02)
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
    ("unknown_flag", "Unknown flag"),
    ("invalid_user", "Cannot evaluate"),
    ("wrong_context_type", "context key 'id' expects number, received string"),
    ("invalid_key", "Initialization failed: check your SDK key"),
    ("missing_key", "Set PARTLY_SDK_KEY"),
    ("invalid_url", "Set PARTLY_SDK_KEY"),
    ("offline", "Initialization failed: no usable policy"),
])
async def test_dashboard_python_example(server, admin, tmp_path, scenario, expected):
    _, key = make_key(admin)
    save_flag(admin, enabled=scenario != "disabled", percentage=50 if scenario == "invalid_user" else 100)
    if scenario == "wrong_context_type":
        save_flag(admin, flag={"mode": "custom", "action": "enable", "groups": [[{"k": "id", "op": "ge", "v": 1}]]})
    response = httpx.get(f"{server.url}/docs/python.py", trust_env=False)
    assert response.status_code == 200
    assert response.text == (ROOT / "examples/python/quickstart.py").read_text()
    script = tmp_path / "quickstart.py"
    script.write_text(response.text)
    env = {**os.environ, "PARTLY_URL": server.url, "PARTLY_SDK_KEY": key,
           "PARTLY_FLAG": "new_checkout", "PARTLY_USER_ID": "user-123"}
    if scenario == "unknown_flag":
        env["PARTLY_FLAG"] = "undefined"
    elif scenario == "invalid_user":
        env["PARTLY_USER_ID"] = ""
    elif scenario == "invalid_key":
        env["PARTLY_SDK_KEY"] = "invalid"
    elif scenario == "missing_key":
        del env["PARTLY_SDK_KEY"]
    elif scenario == "invalid_url":
        env["PARTLY_URL"] = "invalid"
    elif scenario == "offline":
        server.stop()
    process = await asyncio.create_subprocess_exec(
        sys.executable, str(script), env=env,
        stdout=asyncio.subprocess.PIPE, stderr=asyncio.subprocess.PIPE,
    )
    stdout, stderr = await process.communicate()
    assert key not in (stdout + stderr).decode()
    if scenario in ("enabled", "disabled"):
        assert process.returncode == 0, stderr.decode()
        assert expected in stdout.decode()
        assert "Policy updated: False" in stdout.decode()
    else:
        assert process.returncode != 0
        assert expected in stderr.decode()
        assert "Use the" not in stdout.decode()


async def test_quickstart_keeps_policy_after_failed_refresh(server, admin, monkeypatch, capsys):
    _, key = make_key(admin)
    save_flag(admin)
    for name, value in {"PARTLY_URL": server.url, "PARTLY_SDK_KEY": key,
                        "PARTLY_FLAG": "new_checkout", "PARTLY_USER_ID": "user-123"}.items():
        monkeypatch.setenv(name, value)
    original_init = Client.init

    async def init_then_stop_server(client):
        await original_init(client)
        server.stop()

    monkeypatch.setattr(Client, "init", init_then_stop_server)
    example = runpy.run_path(str(ROOT / "examples/python/quickstart.py"))
    await example["main"]()
    captured = capsys.readouterr()
    assert "Refresh failed; keeping the last accepted policy." in captured.err
    assert "Use the new checkout" in captured.out
