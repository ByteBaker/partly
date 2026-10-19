# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Elastic-2.0

import os
import signal
import socket
import ssl
import subprocess
import time
from pathlib import Path

import httpx
import pytest

ROOT = Path(__file__).resolve().parents[2]
PASSWORD = "local-test-password"
ADMIN_HEADERS = {"X-Partly-Request": "dashboard"}


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
    response = admin.post(f"/api/environments/{environment}/keys", json={"name": name})
    assert response.status_code == 200
    return response.json()["id"], response.json()["secret"]


def save_flag(admin, percentage=100, enabled=True, environment="development", key="new_checkout", flag=None):
    if flag is None:
        flag = {"mode": "percentage", "percentage": percentage} if enabled else {"mode": "disabled"}
    path = f"/api/environments/{environment}/flags"
    response = admin.post(path, json={"key": key, **flag})
    if response.status_code == 409:
        response = admin.put(f"{path}/{key}", json=flag)
        assert response.status_code == 200, response.text
    else:
        assert response.status_code == 201, response.text
    assert response.json() == {"key": key, **flag}
    return response.json()
