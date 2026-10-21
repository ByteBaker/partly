# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Elastic-2.0

import os
import re
import ssl
import subprocess
from datetime import datetime, timezone
from pathlib import Path

import httpx
import pytest
import trustme
from conftest import (
    ADMIN_HEADERS,
    PASSWORD,
    ROOT,
    Server,
    keys,
    make_key,
    save_flag,
    scope_for,
)
from playwright.sync_api import expect, sync_playwright
from test_dashboard import create_flag, create_key, login

from partly import Client, Decision, PolicyNotReadyError, RefreshError


def write_certificate(certificate, cert_path, key_path):
    cert_path.write_bytes(b"".join(pem.bytes() for pem in certificate.cert_chain_pems))
    certificate.private_key_pem.write_to_path(key_path)


@pytest.fixture
def tls_files(tmp_path):
    ca = trustme.CA()
    files = {"ca": ca, "bundle": tmp_path / "ca.pem", "cert": tmp_path / "server.pem", "key": tmp_path / "server-key.pem"}
    ca.cert_pem.write_to_path(files["bundle"])
    write_certificate(ca.issue_cert("127.0.0.1", "localhost", "partly.test"), files["cert"], files["key"])
    return files


@pytest.fixture
def tls_server(tmp_path, tls_files):
    server = Server(tmp_path, tls_cert=tls_files["cert"], tls_key=tls_files["key"], ca_bundle=tls_files["bundle"], hostname="partly.test")
    try:
        server.start()
        yield server
    finally:
        server.stop()


def sdk_client(server, key, ca_bundle=None):
    builder = Client.builder().url(server.url).environment(scope_for(key)[0]).application_id(scope_for(key)[1]).secret_key(key).poll_interval(60).timeout(2)
    if ca_bundle is not None:
        builder.ca_bundle(ca_bundle)
    return builder.build()


def test_https_setup_cookies_origins_and_hostname(tls_server):
    server = tls_server
    with httpx.Client(base_url=server.url, headers=ADMIN_HEADERS, verify=server.verify, trust_env=False) as admin:
        assert admin.get("/api/status").json()["setup_required"]
        assert not server.db.exists()
        credentials = {"username": "admin", "password": PASSWORD}
        setup = admin.post("/api/setup", json=credentials, headers={"Origin": server.url})
        assert setup.status_code == 200, setup.text
        assert all(attribute in setup.headers["set-cookie"] for attribute in ["Secure", "HttpOnly", "SameSite=Strict"])
        assert admin.get("/api/status").json()["authenticated"]
        _, key = make_key(admin)
        for headers in [
            {"Origin": server.url.replace("https:", "http:")},
            {"Origin": "https://other.example"},
            {"Origin": server.url.replace("https:", "http:"), "X-Forwarded-Proto": "http"},
            {"Origin": server.url.replace("https:", "http:"), "Forwarded": f'proto=http;host="127.0.0.1:{server.port}"'},
        ]:
            response = admin.post("/api/environments/development/apps", json={"name": "Rejected"}, headers=headers)
            assert response.status_code == 403
            assert response.headers["cache-control"] == "no-store"
        assert len(keys(admin, "development")) == 1
        assert admin.get("/api/status", headers={"Host": f"partly.test:{server.port}"}).status_code == 200
        assert admin.get("/api/status", headers={"Host": f"partly.test:{server.port}", "Origin": f"https://partly.test:{server.port}"}).status_code == 200
        assert admin.get("/api/status", headers={"Host": "unknown.example", "X-Forwarded-Host": "partly.test"}).status_code == 421
        old_cookie = f"partly_session={admin.cookies['partly_session']}"
        logout = admin.post("/api/logout", headers={"Origin": server.url})
        assert logout.status_code == 204 and "Secure" in logout.headers["set-cookie"]
        assert admin.get("/api/environments").status_code == 401
        assert admin.get("/api/environments", headers={"Cookie": old_cookie}).status_code == 401
        signed_in = admin.post("/api/login", json=credentials, headers={"Origin": server.url})
        assert signed_in.status_code == 200 and "Secure" in signed_in.headers["set-cookie"]
        assert admin.get("/api/environments").status_code == 200
    assert key not in (server.directory / "server.log").read_text()
    with pytest.raises(httpx.HTTPError):
        httpx.get(server.url.replace("https:", "http:"), trust_env=False, timeout=1)


@pytest.mark.parametrize("version, negotiated", [(ssl.TLSVersion.TLSv1_2, "TLSv1.2"), (ssl.TLSVersion.TLSv1_3, "TLSv1.3")])
def test_https_supports_verified_tls_versions(tls_server, tls_files, version, negotiated):
    verify = ssl.create_default_context(cafile=str(tls_files["bundle"]))
    verify.minimum_version = verify.maximum_version = version
    with httpx.Client(base_url=tls_server.url, verify=verify, trust_env=False) as client:
        with client.stream("GET", "/api/status") as response:
            assert response.status_code == 200
            tls = response.extensions["network_stream"].get_extra_info("ssl_object")
            assert tls.version() == negotiated


async def test_https_sdk_trust_refresh_and_outage(tls_server, tls_files, monkeypatch):
    server = tls_server
    admin = server.admin()
    _, key = make_key(admin)
    save_flag(admin, flag={"mode": "enabled"})
    # Trust must come from the builder, not a process environment override.
    monkeypatch.setenv("SSL_CERT_FILE", str(tls_files["bundle"]))
    untrusted = sdk_client(server, key)
    trusted = sdk_client(server, key, tls_files["bundle"])
    try:
        with pytest.raises(RefreshError):
            await untrusted.init()
        with pytest.raises(PolicyNotReadyError):
            untrusted.evaluate("new_checkout", context={})
        await trusted.init()
        assert trusted.evaluate("new_checkout", context={}) is Decision.ENABLED
        assert await trusted.refresh() is False
        save_flag(admin, flag={"mode": "disabled"})
        assert await trusted.refresh() is True
        assert trusted.evaluate("new_checkout", context={}) is Decision.DISABLED
        snapshot, etag = trusted._snapshot, trusted._etag
        server.stop()
        with pytest.raises(RefreshError):
            await trusted.refresh()
        assert trusted._snapshot is snapshot and trusted._etag == etag
        assert trusted.evaluate("new_checkout", context={}) is Decision.DISABLED
    finally:
        admin.close()
        await untrusted.aclose()
        await trusted.aclose()


async def test_https_client_never_downgrades_to_plain_http(server, admin):
    _, key = make_key(admin)
    save_flag(admin, flag={"mode": "enabled"})
    client = Client.builder().url(server.url.replace("http:", "https:")).environment(scope_for(key)[0]).application_id(scope_for(key)[1]).secret_key(key).timeout(1).build()
    try:
        with pytest.raises(RefreshError):
            await client.init()
        with pytest.raises(PolicyNotReadyError):
            client.evaluate("new_checkout", context={})
    finally:
        await client.aclose()


@pytest.mark.parametrize("certificate_problem", ["wrong_host", "expired", "untrusted_issuer"])
async def test_invalid_tls_certificates_fail_closed_and_preserve_policy(tls_server, tls_files, certificate_problem):
    server = tls_server
    admin = server.admin()
    _, key = make_key(admin)
    save_flag(admin, flag={"mode": "enabled"})
    warm = sdk_client(server, key, tls_files["bundle"])
    cold = sdk_client(server, key, tls_files["bundle"])
    try:
        await warm.init()
        snapshot, etag = warm._snapshot, warm._etag
        server.stop()
        if certificate_problem == "wrong_host":
            certificate = tls_files["ca"].issue_cert("different.example")
        elif certificate_problem == "expired":
            certificate = tls_files["ca"].issue_cert(
                "127.0.0.1", not_before=datetime(2000, 1, 1, tzinfo=timezone.utc), not_after=datetime(2001, 1, 1, tzinfo=timezone.utc),
            )
        else:
            certificate = trustme.CA().issue_cert("127.0.0.1")
        write_certificate(certificate, tls_files["cert"], tls_files["key"])
        # Readiness checks only TCP here; the clients under test must reject TLS.
        server.start(wait_for_http=False)
        with pytest.raises(RefreshError):
            await cold.init()
        assert cold._poll_task is None
        with pytest.raises(PolicyNotReadyError):
            cold.evaluate("new_checkout", context={})
        with pytest.raises(RefreshError):
            await warm.refresh()
        assert warm._snapshot is snapshot and warm._etag == etag
        assert warm.evaluate("new_checkout", context={}) is Decision.ENABLED
        assert key not in str(cold.last_refresh_error)
        assert key not in str(warm.last_refresh_error)
    finally:
        admin.close()
        await warm.aclose()
        await cold.aclose()


@pytest.mark.parametrize("configuration_problem", ["missing_cert", "missing_key", "malformed_cert", "malformed_key", "mismatched_key"])
def test_invalid_tls_configuration_preserves_missing_database(tmp_path, tls_files, configuration_problem):
    if configuration_problem == "malformed_cert":
        tls_files["cert"].write_text("not a PEM certificate")
    elif configuration_problem == "malformed_key":
        tls_files["key"].write_text("not a PEM private key")
    elif configuration_problem == "mismatched_key":
        tls_files["ca"].issue_cert("127.0.0.1").private_key_pem.write_to_path(tls_files["key"])
    binary = Path(os.environ.get("PARTLY_BINARY", ROOT / "target/debug/partly")).resolve()
    database = tmp_path / "uncreated.db"
    command = [str(binary), "serve", "--port", "0", "--db", str(database)]
    if configuration_problem != "missing_cert":
        command += ["--tls-cert", str(tls_files["cert"])]
    if configuration_problem != "missing_key":
        command += ["--tls-key", str(tls_files["key"])]
    result = subprocess.run(command, capture_output=True, text=True, timeout=10)
    assert result.returncode != 0
    assert result.stderr
    assert not database.exists()
    assert tls_files["key"].read_text() not in result.stderr


def test_https_dashboard_setup_login_flags_and_keys(tls_server):
    server = tls_server
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch()
        # Only this browser context accepts the temporary CA. SDK tests verify TLS.
        context = browser.new_context(ignore_https_errors=True)
        page = context.new_page()
        errors = []
        writes = []
        page.on("pageerror", lambda error: errors.append(str(error)))
        page.on("request", lambda request: writes.append(request) if request.method in ("POST", "PUT", "DELETE") else None)
        try:
            page.goto(server.url)
            expect(page.get_by_role("button", name="Create admin account")).to_be_visible()
            assert not server.db.exists()
            page.get_by_label("Username", exact=True).fill("admin")
            page.get_by_label("Password", exact=True).fill(PASSWORD)
            page.get_by_role("button", name="Create admin account").click()
            expect(page).to_have_url(f"{server.url}/environments/development/flags")
            assert page.evaluate("performance.getEntriesByType('navigation')[0].nextHopProtocol") == "h2"
            session = next(cookie for cookie in context.cookies() if cookie["name"] == "partly_session")
            assert session["secure"] and session["httpOnly"] and session["sameSite"] == "Strict"
            create_flag(page, "new_checkout", "enabled")
            page.get_by_role("tab", name="Apps & Keys").click()
            create_key(page, "HTTPS application")
            expect(page.locator(".key-controls input")).to_have_value(re.compile(r"pk_[A-Za-z0-9]{4}•+[A-Za-z0-9]{4}"))
            page.get_by_role("button", name="Sign out").click()
            expect(page.get_by_role("button", name="Sign in", exact=True)).to_be_visible()
            login(page, server, screen="apps")
            expect(page.get_by_label("Name for HTTPS application", exact=True)).to_be_visible()
            assert writes
            for request in writes:
                assert request.url.startswith(f"{server.url}/api/")
                assert request.all_headers()["origin"] == server.url
            assert not errors
        finally:
            context.close()
            browser.close()
