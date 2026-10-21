# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Elastic-2.0

import json
import os
import re
import signal
import socket
import ssl
import subprocess
import time
from pathlib import Path

import httpx
import pytest

ROOT = Path(__file__).resolve().parents[2]
RULE_CASES = json.loads((ROOT / "tests/sdk_contract/rules.json").read_text())
PASSWORD = "local-test-password"
ADMIN_HEADERS = {"X-Partly-Request": "dashboard"}
KEY_SCOPES = {}


def scope_for(key):
    return KEY_SCOPES.get(key, ("development", "app_test"))


def policy_headers(key):
    environment, application_id = scope_for(key)
    return {"Authorization": f"Bearer {key}", "X-Partly-Environment": environment, "X-Partly-Application": application_id}


def app_id(admin, environment="development"):
    return admin.get(f"/api/environments/{environment}/apps").json()["applications"][0]["id"]


def keys(admin, environment="development"):
    return [key for app in admin.get(f"/api/environments/{environment}/apps").json()["applications"] for key in app["keys"]]


def confirm_request(admin, request):
    """Existing workflow tests confirm edits; dedicated confirmation tests supply their own headers."""
    match = re.fullmatch(r"/api/environments/([^/]+)/flags/([^/]+)", request.url.path)
    if match and request.method in ("PUT", "DELETE") and "x-partly-confirm" not in request.headers:
        response = admin.get(f"{request.url.path}/impact")
        request.headers["X-Partly-Confirm"] = f"{match[1]}/{match[2]}"
        if response.status_code == 200:
            request.headers["X-Partly-Impact"] = response.json()["token"]



class Server:
    def __init__(self, directory, *, tls_cert=None, tls_key=None, ca_bundle=None, hostname=None, host="127.0.0.1"):
        self.directory = directory
        self.db = directory / "partly.db"
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            self.port = sock.getsockname()[1]
        self.tls_cert = tls_cert
        self.tls_key = tls_key
        self.hostname = hostname
        self.host = host
        self.verify = ssl.create_default_context(cafile=str(ca_bundle)) if ca_bundle else True
        scheme = "https" if tls_cert else "http"
        self.url = f"{scheme}://127.0.0.1:{self.port}"
        self.process = None
        self.log = None

    def start(self, *, wait_for_http=True):
        binary = Path(os.environ.get("PARTLY_BINARY", ROOT / "target/debug/partly")).resolve()
        self.log = (self.directory / "server.log").open("ab")
        command = [str(binary), "serve", "--host", self.host, "--port", str(self.port), "--db", str(self.db)]
        for option, value in [("--tls-cert", self.tls_cert), ("--tls-key", self.tls_key), ("--hostname", self.hostname)]:
            if value is not None:
                command.extend([option, str(value)])
        self.process = subprocess.Popen(
            command,
            cwd=self.directory, stdout=self.log, stderr=self.log,
        )
        deadline = time.monotonic() + 10
        with httpx.Client(trust_env=False, timeout=0.3, verify=self.verify) as client:
            while time.monotonic() < deadline:
                if self.process.poll() is not None:
                    raise RuntimeError((self.directory / "server.log").read_text())
                try:
                    if not wait_for_http:
                        with socket.create_connection(("127.0.0.1", self.port), timeout=0.3):
                            return
                    if client.get(f"{self.url}/api/status").status_code == 200:
                        return
                except (httpx.HTTPError, OSError):
                    pass
                time.sleep(0.02)
        raise RuntimeError("Partly did not start")

    def stop(self):
        if self.process and self.process.poll() is None:
            self.process.send_signal(signal.SIGINT)
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
        if self.log:
            self.log.close()

    def admin(self, setup=True):
        client = httpx.Client(base_url=self.url, headers=ADMIN_HEADERS, trust_env=False, timeout=10, verify=self.verify)
        response = client.post("/api/setup" if setup else "/api/login", json={"username": "admin", "password": PASSWORD})
        assert response.status_code == 200, response.text
        client.event_hooks["request"].append(lambda request: confirm_request(client, request))
        return client


@pytest.fixture
def server(tmp_path):
    instance = Server(tmp_path)
    try:
        instance.start()
        yield instance
    finally:
        instance.stop()


@pytest.fixture
def admin(server):
    client = server.admin()
    try:
        yield client
    finally:
        client.close()


def make_key(admin, environment="development", name="Test application"):
    apps = admin.get(f"/api/environments/{environment}/apps").json()["applications"]
    if apps:
        application = apps[0]
        response = admin.post(f"/api/environments/{environment}/apps/{application['id']}/keys", json={"name": name})
        assert response.status_code == 200, response.text
        key = response.json()
    else:
        flags = list(admin.get(f"/api/environments/{environment}/flags").json())
        response = admin.post(f"/api/environments/{environment}/apps", json={"name": "Test application", "flags": flags})
        assert response.status_code == 201, response.text
        application = response.json()
        key = application["keys"][0]
        assert admin.put(f"/api/keys/{key['id']}", json={"name": name}).status_code == 204
    KEY_SCOPES[key["secret"]] = (environment, application["id"])
    return key["id"], key["secret"]


def save_flag(admin, percentage=100, enabled=True, environment="development", key="new_checkout", flag=None):
    if flag is None:
        flag = {"mode": "percentage", "percentage": percentage} if enabled else {"mode": "disabled"}
    path = f"/api/environments/{environment}/flags"
    response = admin.post(path, json={"key": key, **flag})
    if response.status_code == 409:
        impact = admin.get(f"{path}/{key}/impact").json()
        response = admin.put(f"{path}/{key}", json=flag, headers={"X-Partly-Confirm": f"{environment}/{key}", "X-Partly-Impact": impact["token"]})
        assert response.status_code == 200, response.text
    else:
        assert response.status_code == 201, response.text
    assert response.json() == {"key": key, **flag}
    # This fixture application explicitly subscribes to flags created by the fixture.
    for app in admin.get(f"/api/environments/{environment}/apps").json()["applications"]:
        if app["name"] == "Test application" and key not in app["flags"]:
            assigned = admin.put(f"/api/environments/{environment}/apps/{app['id']}", json={"name": app["name"], "flags": [*app["flags"], key]})
            assert assigned.status_code == 200, assigned.text
    return response.json()
