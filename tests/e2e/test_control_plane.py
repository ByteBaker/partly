# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Elastic-2.0

import concurrent.futures
import hashlib
import json
import os
import re
import secrets
import sqlite3
import subprocess
from pathlib import Path

import httpx
import pytest
from conftest import (
    ADMIN_HEADERS,
    KEY_SCOPES,
    PASSWORD,
    ROOT,
    app_id,
    keys,
    make_key,
    policy_headers,
    save_flag,
)


def test_delayed_setup_and_restart(server):
    with httpx.Client(base_url=server.url, headers=ADMIN_HEADERS, trust_env=False) as client:
        assert client.get("/").status_code == 200
        assert client.get("/api/status").json()["setup_required"]
        assert not server.db.exists()
        assert client.post("/api/setup", json={"username": "admin", "password": "short"}).status_code == 400
        assert not server.db.exists()
        server.stop()
        assert not server.db.exists()
        server.start()
        response = client.post("/api/setup", json={"username": "admin", "password": PASSWORD})
        assert response.status_code == 200
        assert server.db.exists()
        assert "HttpOnly" in response.headers["set-cookie"]
        assert "SameSite=Strict" in response.headers["set-cookie"]
        assert client.post("/api/setup", json={"username": "other", "password": PASSWORD}).status_code == 409
        assert not client.get("/api/status").json()["setup_required"]
        save_flag(client, percentage=25)
        key_id, key = make_key(client)
        response = client.get("/api/environments/development/apps")
        assert [(item["id"], item["name"], item["secret"]) for item in response.json()["applications"][0]["keys"]] == [(key_id, "Test application", key)]
        assert response.headers["cache-control"] == "no-store"
        assert client.get(f"/api/keys/{key_id}/reveal").status_code == 404
        with sqlite3.connect(server.db) as db:
            stored = db.execute("SELECT password_hash FROM users").fetchone()[0]
            assert stored.startswith("$argon2id$") and PASSWORD not in stored
        if os.name == "posix":
            assert server.db.stat().st_mode & 0o777 == 0o600
        server.stop()
        server.start()
        assert client.get("/api/environments/development/apps").status_code == 401
        assert client.post("/api/login", json={"username": "admin", "password": "wrong"}).status_code == 401
        assert client.post("/api/login", json={"username": "admin", "password": PASSWORD}).status_code == 200
        assert keys(client, "development")[0]["secret"] == key
        assert client.get("/api/environments/development/flags").json()["new_checkout"]["percentage"] == 25
        assert key not in (server.directory / "server.log").read_text()
        old_cookie = f"partly_session={client.cookies['partly_session']}"
        assert client.post("/api/logout").status_code == 204
        assert client.get("/api/environments").status_code == 401
        assert client.get("/api/environments", headers={"Cookie": old_cookie}).status_code == 401


def test_concurrent_setup_creates_only_one_admin(server):
    def submit(username):
        return httpx.post(f"{server.url}/api/setup", headers=ADMIN_HEADERS, json={"username": username, "password": PASSWORD}, timeout=10, trust_env=False).status_code
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
        results = sorted(pool.map(submit, ["first", "second"]))
    assert results == [200, 409]
    with sqlite3.connect(server.db) as db:
        assert db.execute("SELECT COUNT(*) FROM users").fetchone()[0] == 1


@pytest.mark.parametrize("content", [b"not sqlite\x00", b""])
def test_invalid_existing_database_is_preserved(tmp_path, content):
    db = tmp_path / "existing.db"
    db.write_bytes(content)
    before = hashlib.sha256(db.read_bytes()).hexdigest()
    binary = Path(os.environ.get("PARTLY_BINARY", ROOT / "target/debug/partly")).resolve()
    result = subprocess.run([str(binary), "serve", "--port", "0", "--db", str(db)], capture_output=True, text=True, timeout=10)
    assert result.returncode != 0
    assert "could not start" in result.stderr
    assert hashlib.sha256(db.read_bytes()).hexdigest() == before


def test_authentication_scopes_and_conditional_policy(server, admin):
    dev_id, dev = make_key(admin)
    _, prod = make_key(admin, "production")
    save_flag(admin, percentage=30)
    save_flag(admin, percentage=100, enabled=False, environment="production")
    with httpx.Client(base_url=server.url, trust_env=False) as sdk:
        assert sdk.get("/api/environments").status_code == 401
        assert sdk.get("/api/environments/development/apps").status_code == 401
        assert sdk.put(f"/api/keys/{dev_id}", headers=ADMIN_HEADERS, json={"name": "Unauthorized"}).status_code == 401
        assert sdk.post("/api/setup", json={}).status_code == 403
        assert sdk.get("/api/status", headers={"Sec-Fetch-Site": "cross-site"}).status_code == 403
        assert sdk.get("/api/status", headers={"Host": "rebind.example"}).status_code == 421
        assert admin.post("/api/environments/development/apps", headers={"Origin": "http://evil.example"}).status_code == 403
        assert sdk.get("/api/policy").status_code == 401
        sdk.cookies.update(admin.cookies)
        assert sdk.get("/api/policy").status_code == 401
        sdk.cookies.clear()
        sdk.headers.update(policy_headers(dev))
        assert sdk.get("/api/environments").status_code == 401
        assert sdk.get("/api/environments/development/apps").status_code == 401
        first = sdk.get("/api/policy")
        assert first.status_code == 200
        assert first.json()["environment"] == "development"
        assert first.json()["flags"]["new_checkout"]["percentage"] == 30
        sdk.headers["If-None-Match"] = first.headers["etag"]
        unchanged = sdk.get("/api/policy")
        assert unchanged.status_code == 304 and unchanged.content == b""
        save_flag(admin, percentage=30)
        assert sdk.get("/api/policy").status_code == 304
        save_flag(admin, percentage=60, environment="production")
        assert sdk.get("/api/policy").status_code == 304
        save_flag(admin, percentage=60)
        updated = sdk.get("/api/policy")
        assert updated.status_code == 200 and updated.headers["etag"] != first.headers["etag"]
        production = sdk.get("/api/policy", headers=policy_headers(prod))
        assert production.json()["environment"] == "production"
        assert admin.delete(f"/api/keys/{dev_id}").status_code == 204
        assert sdk.get("/api/policy", headers={"If-None-Match": updated.headers["etag"]}).status_code == 401
    assert admin.put("/api/environments/development/flags/INVALID", json={"mode": "percentage", "percentage": 10}).status_code == 400
    assert admin.put("/api/environments/development/flags/valid", json={"mode": "percentage", "percentage": 101}).status_code == 400
    assert admin.put("/api/environments/development/flags/valid", json={"mode": "percentage", "percentage": "10"}).status_code == 422
    assert admin.get("/api/environments/unknown/flags").status_code == 404
    assert admin.delete("/api/environments/development/flags/new_checkout").status_code == 204
    assert admin.get("/api/environments/development/flags").json() == {}


def test_empty_policy_is_a_document(admin, server):
    _, key = make_key(admin)
    result = httpx.get(f"{server.url}/api/policy", headers=policy_headers(key), trust_env=False)
    assert result.status_code == 200
    assert result.json()["flags"] == {}
    assert result.headers["etag"]


def test_named_short_and_long_keys_survive_restart(server, admin):
    new_id, new_key = make_key(admin)
    save_flag(admin, percentage=25)
    assert re.fullmatch(r"pk_[A-Za-z0-9]{24}", new_key)
    legacy_key = f"pk_{secrets.token_hex(32)}"
    application = app_id(admin)
    KEY_SCOPES[legacy_key] = ("development", application)
    server.stop()
    with sqlite3.connect(server.db) as db:
        db.execute("INSERT INTO sdk_keys (id, application_id, secret, secret_hash, masked_secret, name) VALUES (?, ?, ?, ?, ?, ?)", ("legacy-key", application, legacy_key, hashlib.sha256(legacy_key.encode()).hexdigest(), legacy_key[:7] + "••••" + legacy_key[-4:], "Long key"))
    server.start()
    response = admin.post("/api/login", json={"username": "admin", "password": PASSWORD})
    assert response.status_code == 200
    records = keys(admin, "development")
    assert [record["name"] for record in records] == ["Test application", "Long key"]
    with sqlite3.connect(server.db) as db:
        assert db.execute("PRAGMA user_version").fetchone()[0] == 1
    for key_id, key in [(new_id, new_key), ("legacy-key", legacy_key)]:
        assert next(record["secret"] for record in records if record["id"] == key_id) == key
        response = httpx.get(f"{server.url}/api/policy", headers=policy_headers(key), trust_env=False)
        assert response.status_code == 200
        assert response.json()["environment"] == "development"
        assert response.json()["flags"]["new_checkout"]["percentage"] == 25
    assert admin.put(f"/api/keys/{new_id}", json={"name": "Checkout service"}).status_code == 204
    server.stop()
    server.start()
    assert admin.post("/api/login", json={"username": "admin", "password": PASSWORD}).status_code == 200
    assert keys(admin, "development")[0]["name"] == "Checkout service"
    assert admin.delete("/api/keys/legacy-key").status_code == 204
    assert httpx.get(f"{server.url}/api/policy", headers=policy_headers(legacy_key), trust_env=False).status_code == 401
    assert httpx.get(f"{server.url}/api/policy", headers=policy_headers(new_key), trust_env=False).status_code == 200


def test_key_names_validation_and_environment_isolation(admin):
    key_id, key = make_key(admin, name="  Checkout service  ")
    key_path = f"/api/environments/development/apps/{app_id(admin)}/keys"
    for name in ["", "   ", "x" * 65, "two\nlines", "null\x00character"]:
        assert admin.post(key_path, json={"name": name}).status_code == 400
        assert admin.put(f"/api/keys/{key_id}", json={"name": name}).status_code == 400
    assert admin.post(key_path, json={}).status_code == 422
    assert admin.post(key_path, json={"name": "Wrong scope", "environment": "production"}).status_code == 422
    assert admin.put(f"/api/keys/{key_id}", json={"name": "Wrong scope", "environment": "production"}).status_code == 422
    assert admin.put("/api/keys/missing", json={"name": "Checkout"}).status_code == 404
    assert keys(admin, "production") == []
    records = keys(admin, "development")
    assert [(item["id"], item["name"], item["secret"]) for item in records] == [(key_id, "Checkout service", key)]
    assert admin.put(f"/api/keys/{key_id}", json={"name": "Checkout worker"}).status_code == 204
    assert [(item["id"], item["name"], item["secret"]) for item in keys(admin)] == [(key_id, "Checkout worker", key)]


@pytest.mark.parametrize("legacy_change", [
    "ALTER TABLE sdk_keys DROP COLUMN name",
    "ALTER TABLE users RENAME TO admin",
    "DROP TABLE permissions",
    "DROP TABLE flags; CREATE TABLE flags (environment TEXT, key TEXT, enabled INTEGER, percentage INTEGER)",
])
def test_earlier_development_schema_is_rejected_without_changes(server, admin, legacy_change):
    make_key(admin)
    server.stop()
    with sqlite3.connect(server.db) as db:
        db.executescript(legacy_change)
    before = hashlib.sha256(server.db.read_bytes()).hexdigest()
    binary = Path(os.environ.get("PARTLY_BINARY", ROOT / "target/debug/partly")).resolve()
    result = subprocess.run([str(binary), "serve", "--port", "0", "--db", str(server.db)], capture_output=True, text=True, timeout=10)
    assert result.returncode != 0
    assert "could not start" in result.stderr
    assert "incompatible development schema" in result.stderr
    assert hashlib.sha256(server.db.read_bytes()).hexdigest() == before


def test_dashboard_environment_urls(server):
    with httpx.Client(base_url=server.url, trust_env=False) as client:
        for environment in ["development", "production"]:
            for screen in ["flags", "apps", "audits", "docs"]:
                response = client.get(f"/environments/{environment}/{screen}")
                assert response.status_code == 200
                assert response.headers["content-type"].startswith("text/html")
        assert client.get("/environments/unknown/flags").status_code == 404
        assert client.get("/environments/production/unknown").status_code == 404
        assert not server.db.exists()


def test_flag_creation_updates_and_json_persistence(server, admin):
    path = "/api/environments/development/flags"
    configs = {
        "always": {"mode": "enabled"},
        "never": {"mode": "disabled"},
        "gradual": {"mode": "percentage", "percentage": 37},
        "eligible": {
            "mode": "custom", "action": "enable",
            "groups": [[{"k": "age", "op": "ge", "v": 18}, {"k": "plan", "op": "matches", "v": "pro"}],
                       [{"k": "staff", "op": "is", "v": True}]],
        },
        "excluded": {"mode": "custom", "action": "disable", "groups": [[{"k": "email", "op": "regex", "v": "@example\\.com$"}]]},
    }
    for key, flag in configs.items():
        response = admin.post(path, json={"key": key, **flag})
        assert response.status_code == 201, response.text
        assert response.json() == {"key": key, **flag}
    assert admin.get(path).json() == configs
    assert admin.post(path, json={"key": "gradual", "mode": "disabled"}).status_code == 409
    assert admin.put(f"{path}/absent", json={"mode": "enabled"}).status_code == 404
    assert admin.post("/api/environments/unknown/flags", json={"key": "always", "mode": "enabled"}).status_code == 404
    assert admin.get("/api/environments/production/flags").json() == {}
    _, secret = make_key(admin)
    with httpx.Client(base_url=server.url, headers=policy_headers(secret), trust_env=False) as sdk:
        first = sdk.get("/api/policy")
        assert first.json()["revision"] >= 1
        sdk.headers["If-None-Match"] = first.headers["etag"]
        unchanged = admin.put(f"{path}/eligible", json=configs["eligible"])
        assert unchanged.status_code == 200 and unchanged.json() == {"key": "eligible", **configs["eligible"]}
        assert sdk.get("/api/policy").status_code == 304
        configs["gradual"] = {"mode": "enabled"}
        updated = admin.put(f"{path}/gradual", json=configs["gradual"])
        assert updated.status_code == 200 and updated.json() == {"key": "gradual", "mode": "enabled"}
        changed = sdk.get("/api/policy")
        assert changed.status_code == 200
        assert changed.json()["revision"] == first.json()["revision"] + 1
        assert changed.json()["flags"] == configs
    with sqlite3.connect(server.db) as db:
        assert db.execute("PRAGMA user_version").fetchone()[0] == 1
        assert {key: json.loads(config) for key, config in db.execute("SELECT key, config FROM flags")} == configs
    server.stop()
    server.start()
    assert admin.post("/api/login", json={"username": "admin", "password": PASSWORD}).status_code == 200
    assert admin.get(path).json() == configs


@pytest.mark.parametrize("flag", [
    {"mode": "unknown"},
    {"mode": "enabled", "percentage": 50},
    {"mode": "percentage", "percentage": -1},
    {"mode": "percentage", "percentage": 101},
    {"mode": "custom", "action": "enable", "groups": []},
    {"mode": "custom", "action": "enable", "groups": [[]]},
    {"mode": "custom", "action": "other", "groups": [[{"k": "age", "op": "ge", "v": 18}]]},
    *[{"mode": "custom", "action": "enable", "groups": [[condition]]} for condition in [
        {"k": "age", "op": "other", "v": 18},
        {"k": "age", "op": "ge", "v": "18"},
        {"k": "age", "op": "eq", "v": True},
        {"k": "age", "op": "eq", "v": "18"},
        {"k": "plan", "op": "prefix", "v": 1},
        {"k": "plan", "op": "matches", "v": False},
        {"k": "active", "op": "is", "v": 1},
        {"k": "active", "op": "is", "v": "true"},
        {"k": "age", "op": "gt", "v": None},
        {"k": "age", "op": "gt", "v": []},
        {"k": "age", "op": "gt", "v": {}},
        {"k": "email", "op": "regex", "v": "["},
        {"k": "email", "op": "regex", "v": "(?i)example"},
        {"k": "email", "op": "regex", "v": "(?=example)"},
        {"k": "age", "op": "gt", "v": 18, "extra": True},
    ]],
])
def test_invalid_flags_do_not_change_policy(admin, server, flag):
    path = "/api/environments/development/flags"
    assert admin.post(path, json={"key": "existing", "mode": "enabled"}).status_code == 201
    _, secret = make_key(admin)
    with httpx.Client(base_url=server.url, headers=policy_headers(secret), trust_env=False) as sdk:
        initial = sdk.get("/api/policy")
        for response in [admin.post(path, json={"key": "invalid", **flag}), admin.put(f"{path}/existing", json=flag)]:
            assert response.status_code in (400, 422), response.text
        assert admin.get(path).json() == {"existing": {"mode": "enabled"}}
        unchanged = sdk.get("/api/policy", headers={"If-None-Match": initial.headers["etag"]})
        assert unchanged.status_code == 304 and not unchanged.content


def test_flag_authentication_precedes_rule_parsing(server, admin):
    path = "/api/environments/development/flags"
    invalid = {"mode": "custom", "action": "enable", "groups": [[{"k": "email", "op": "regex", "v": "["}]]}
    with httpx.Client(base_url=server.url, headers=ADMIN_HEADERS, trust_env=False) as outsider:
        assert outsider.post(path, json={"key": "invalid", **invalid}).status_code == 401
        assert outsider.put(f"{path}/invalid", json=invalid).status_code == 401
    assert admin.post(path, json={"key": "invalid", **invalid}).status_code == 422
    assert admin.post(path, content='{"key":"duplicate","mode":"enabled","mode":"disabled"}', headers={"Content-Type": "application/json"}).status_code == 422
    assert admin.post(path, content='{', headers={"Content-Type": "application/json"}).status_code == 400
    assert admin.post(path, content='{"key":"wrong_type","mode":"enabled"}', headers={"Content-Type": "text/plain"}).status_code == 415
    response = admin.post(path, content='{"key":"vendor_type","mode":"enabled"}', headers={"Content-Type": "application/vnd.partly+json; charset=utf-8"})
    assert response.status_code == 201


def test_policy_size_limit_rejects_write_without_changing_snapshot(server, admin):
    path = "/api/environments/development/flags"
    _, secret = make_key(admin)
    flag = {"mode": "custom", "action": "enable", "groups": [[{"k": "text", "op": "matches", "v": "x" * 15000}]]}
    with httpx.Client(base_url=server.url, headers=policy_headers(secret), trust_env=False) as sdk:
        previous = sdk.get("/api/policy")
        for index in range(75):
            response = admin.post(path, json={"key": f"flag_{index}", **flag})
            if response.status_code == 400:
                assert "1 MiB" in response.json()["error"]
                assert sdk.get("/api/policy", headers={"If-None-Match": previous.headers["etag"]}).status_code == 304
                assert f"flag_{index}" not in admin.get(path).json()
                break
            assert response.status_code == 201, response.text
            previous = sdk.get("/api/policy")
            assert len(previous.content) <= 1024 * 1024
        else:
            pytest.fail("Server accepted a policy above the SDK download limit")
        assert admin.post(path, json={"key": "small", "mode": "enabled"}).status_code == 201
        before_update = sdk.get("/api/policy")
        rejected = admin.put(f"{path}/small", json=flag)
        assert rejected.status_code == 400 and "1 MiB" in rejected.json()["error"]
        assert sdk.get("/api/policy", headers={"If-None-Match": before_update.headers["etag"]}).status_code == 304
        assert admin.get(path).json()["small"] == {"mode": "enabled"}
        # A rejected oversized create must not poison later edits or cleanup.
        assert admin.put(f"{path}/flag_0", json={"mode": "disabled"}).status_code == 200
        assert admin.post(path, json={"key": "replacement", **flag}).status_code == 201


@pytest.mark.parametrize("kind, error", [
    ("reference", "invalid references"),
    ("number", "Invalid stored flag configuration"),
    ("oversized", "1 MiB POC limit"),
])
def test_existing_database_with_invalid_data_is_preserved(server, admin, kind, error):
    make_key(admin)
    save_flag(admin)
    server.stop()
    with sqlite3.connect(server.db) as db:
        if kind == "reference":
            db.execute("UPDATE applications SET environment = 'missing'")
        elif kind == "number":
            config = {"mode": "custom", "action": "enable", "groups": [[
                {"k": "number", "op": "eq", "v": -(2**63) - 1},
            ]]}
            db.execute("UPDATE flags SET config = ?", [json.dumps(config)])
        else:
            config = {"mode": "custom", "action": "enable", "groups": [[
                {"k": "text", "op": "matches", "v": "x" * (1024 * 1024)},
            ]]}
            db.execute("UPDATE flags SET config = ?", [json.dumps(config)])
    before = hashlib.sha256(server.db.read_bytes()).hexdigest()
    binary = Path(os.environ.get("PARTLY_BINARY", ROOT / "target/debug/partly")).resolve()
    result = subprocess.run([str(binary), "serve", "--port", "0", "--db", str(server.db)], capture_output=True, text=True, timeout=10)
    assert result.returncode != 0
    assert "could not start" in result.stderr
    assert error in result.stderr
    assert hashlib.sha256(server.db.read_bytes()).hexdigest() == before


def test_rejected_requests_keep_security_headers(server):
    with httpx.Client(base_url=server.url, trust_env=False) as client:
        responses = [
            client.get("/api/status", headers={"Host": "rebind.example"}),
            client.get("/api/status", headers={"Sec-Fetch-Site": "cross-site"}),
            client.post("/api/setup", json={}),
            client.get("/api/status", headers={"Origin": "http://other.example"}),
        ]
    assert [response.status_code for response in responses] == [421, 403, 403, 403]
    for response in responses:
        assert response.json()["error"]
        assert response.headers["cache-control"] == "no-store"
        assert response.headers["x-content-type-options"] == "nosniff"
        assert "frame-ancestors 'none'" in response.headers["content-security-policy"]
