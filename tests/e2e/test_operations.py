import concurrent.futures
import hashlib
import os
import signal
import socket
import sqlite3
import subprocess
from pathlib import Path

import httpx
import pytest
import trustme
from conftest import ADMIN_HEADERS, PASSWORD, ROOT, Server, make_key, save_flag


def command(*arguments, password=None):
    binary = Path(os.environ.get("PARTLY_BINARY", ROOT / "target/debug/partly")).resolve()
    return subprocess.run([str(binary), *map(str, arguments)], input=password, capture_output=True, text=True, timeout=10)


def database_records(path):
    with sqlite3.connect(path) as db:
        return {table: db.execute(f"SELECT * FROM {table} ORDER BY rowid").fetchall() for table in ["projects", "environments", "flags", "sdk_keys"]}


def test_first_setup_requires_local_binding_and_creates_no_empty_files(tmp_path):
    database = tmp_path / "partly.db"
    result = command("serve", "--host", "0.0.0.0", "--port", 0, "--db", database)
    assert result.returncode != 0
    assert "setup locally first" in result.stderr
    assert not database.exists()
    assert not Path(f"{database}.lock").exists()
    result = command("reset-password", "--db", database, password="new-password-123\n")
    assert result.returncode != 0 and "does not exist" in result.stderr
    assert not database.exists() and not Path(f"{database}.lock").exists()
    local = Server(tmp_path)
    remote = Server(tmp_path, host="0.0.0.0")
    try:
        local.start()
        assert not database.exists() and not Path(f"{database}.lock").exists()
        admin = local.admin()
        admin.close()
        local.stop()
        remote.start()
        signed_in = remote.admin(setup=False)
        assert signed_in.get("/api/status").json()["authenticated"]
        signed_in.close()
    finally:
        local.stop()
        remote.stop()


def test_password_reset_requires_offline_database_and_preserves_flags_keys(server, admin):
    save_flag(admin, percentage=37)
    _, secret = make_key(admin)
    before = database_records(server.db)
    new_password = "replacement-password-123"
    locked = command("reset-password", "--db", server.db, password=f"{new_password}\n")
    assert locked.returncode != 0 and "Database is in use" in locked.stderr
    second = command("serve", "--port", 0, "--db", server.db)
    assert second.returncode != 0 and "Database is in use" in second.stderr
    if os.name == "posix":
        alias = server.directory / "alias.db"
        alias.symlink_to(server.db)
        aliased = command("reset-password", "--db", alias, password=f"{new_password}\n")
        assert aliased.returncode != 0 and "Database is in use" in aliased.stderr
        assert not Path(f"{alias}.lock").exists()
    server.stop()
    assert Path(f"{server.db}.lock").read_bytes() == b""
    reset = command("reset-password", "--db", server.db, password=f"{new_password}\n")
    assert reset.returncode == 0, reset.stderr
    assert new_password not in reset.stdout + reset.stderr
    assert database_records(server.db) == before
    with sqlite3.connect(server.db) as db:
        username, hashed = db.execute("SELECT username, password_hash FROM admin").fetchone()
        assert username == "admin" and hashed.startswith("$argon2id$")
        assert new_password not in hashed
        assert db.execute("PRAGMA user_version").fetchone()[0] == 1
    server.start()
    assert admin.get("/api/environments").status_code == 401
    assert admin.post("/api/login", json={"username": "admin", "password": PASSWORD}).status_code == 401
    assert admin.post("/api/login", json={"username": "admin", "password": new_password}).status_code == 200
    with httpx.Client(base_url=server.url, trust_env=False) as sdk:
        response = sdk.get("/api/policy", headers={"Authorization": f"Bearer {secret}"})
        assert response.status_code == 200 and response.json()["flags"]["new_checkout"]["percentage"] == 37
    assert new_password not in (server.directory / "server.log").read_text()


@pytest.mark.parametrize("password", ["short\n", "x" * 1025, "first-password\nsecond-password\n"])
def test_invalid_reset_password_preserves_database(server, admin, password):
    server.stop()
    before = hashlib.sha256(server.db.read_bytes()).digest()
    result = command("reset-password", "--db", server.db, password=password)
    assert result.returncode != 0
    assert password.strip() not in result.stdout + result.stderr
    assert hashlib.sha256(server.db.read_bytes()).digest() == before


def test_reset_rejects_invalid_database_without_changing_it(tmp_path):
    database = tmp_path / "invalid.db"
    database.write_bytes(b"not a database")
    result = command("reset-password", "--db", database, password="replacement-password-123\n")
    assert result.returncode != 0
    assert database.read_bytes() == b"not a database"


@pytest.mark.skipif(os.name != "posix", reason="SIGTERM is a Unix service signal")
@pytest.mark.parametrize("https", [False, True])
@pytest.mark.parametrize("stalled_body", [False, True])
def test_sigterm_stops_cleanly_and_releases_database(tmp_path, https, stalled_body):
    options = {}
    if https:
        ca = trustme.CA()
        certificate = ca.issue_cert("127.0.0.1")
        cert, key, bundle = (tmp_path / name for name in ["cert.pem", "key.pem", "ca.pem"])
        cert.write_bytes(b"".join(pem.bytes() for pem in certificate.cert_chain_pems))
        certificate.private_key_pem.write_to_path(key)
        ca.cert_pem.write_to_path(bundle)
        options = {"tls_cert": cert, "tls_key": key, "ca_bundle": bundle}
    server = Server(tmp_path, **options)
    connection = None
    try:
        server.start()
        admin = server.admin()
        save_flag(admin, flag={"mode": "enabled"})
        admin.close()
        if stalled_body:
            connection = socket.create_connection(("127.0.0.1", server.port), timeout=2)
            if https:
                connection = server.verify.wrap_socket(connection, server_hostname="127.0.0.1")
            connection.sendall((
                f"POST /api/login HTTP/1.1\r\nHost: 127.0.0.1:{server.port}\r\n"
                "X-Partly-Request: dashboard\r\nContent-Type: application/json\r\n"
                "Content-Length: 1000\r\nExpect: 100-continue\r\n\r\n"
            ).encode())
            # The interim response proves the server has started reading this body.
            assert connection.recv(4096).startswith(b"HTTP/1.1 100 Continue")
            connection.sendall(b"{")
        server.process.send_signal(signal.SIGTERM)
        # Keep the incomplete request open until the process has fully exited.
        assert server.process.wait(timeout=5) == 0
        if connection is not None:
            connection.close()
            connection = None
        server.stop()
        server.start()
        signed_in = server.admin(setup=False)
        assert signed_in.get("/api/environments/development/flags").json()["new_checkout"] == {"mode": "enabled"}
        signed_in.close()
    finally:
        if connection is not None:
            connection.close()
        server.stop()


def test_login_throttle_uses_peer_address_and_keeps_policy_available(server, admin):
    _, secret = make_key(admin)
    save_flag(admin, flag={"mode": "enabled"})
    with httpx.Client(base_url=server.url, headers=ADMIN_HEADERS, trust_env=False) as attacker:
        for attempt in range(5):
            response = attacker.post("/api/login", json={"username": "admin", "password": "wrong-password"}, headers={"X-Forwarded-For": f"192.0.2.{attempt}"})
            assert response.status_code == 401
        response = attacker.post("/api/login", json={"username": "admin", "password": PASSWORD}, headers={"X-Forwarded-For": "198.51.100.1"})
        assert response.status_code == 429 and response.headers["retry-after"] == "60"
        assert response.headers["cache-control"] == "no-store"
        assert attacker.get("/api/policy", headers={"Authorization": f"Bearer {secret}"}).status_code == 200
    assert admin.get("/api/environments").status_code == 200


def test_concurrent_logins_and_policy_reads(server, admin):
    _, secret = make_key(admin)
    save_flag(admin, flag={"mode": "enabled"})
    def request(index):
        with httpx.Client(base_url=server.url, trust_env=False, timeout=5) as client:
            if index % 2:
                return "login", client.post("/api/login", headers=ADMIN_HEADERS, json={"username": "admin", "password": "incorrect-password"}).status_code
            return "policy", client.get("/api/policy", headers={"Authorization": f"Bearer {secret}"}).status_code
    with concurrent.futures.ThreadPoolExecutor(max_workers=10) as pool:
        results = list(pool.map(request, range(20)))
    assert all(status == 200 for kind, status in results if kind == "policy")
    assert all(status in (401, 429) for kind, status in results if kind == "login")
    assert any(status == 429 for _, status in results)
