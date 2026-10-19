import os
from pathlib import Path
import signal
import socket
import subprocess
import time

import httpx
import pytest

ROOT = Path(__file__).resolve().parents[2]
PASSWORD = "local-test-password"
ADMIN_HEADERS = {"X-Partly-Request": "dashboard"}


class Server:
    def __init__(self, directory):
        self.directory = directory
        self.db = directory / "partly.db"
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            self.port = sock.getsockname()[1]
        self.url = f"http://127.0.0.1:{self.port}"
        self.process = None
        self.log = None

    def start(self):
        binary = Path(os.environ.get("PARTLY_BINARY", ROOT / "target/debug/partly")).resolve()
        self.log = (self.directory / "server.log").open("ab")
        self.process = subprocess.Popen(
            [str(binary), "serve", "--port", str(self.port), "--db", str(self.db)],
            cwd=self.directory, stdout=self.log, stderr=self.log,
        )
        deadline = time.monotonic() + 10
        with httpx.Client(trust_env=False, timeout=0.3) as client:
            while time.monotonic() < deadline:
                if self.process.poll() is not None:
                    raise RuntimeError((self.directory / "server.log").read_text())
                try:
                    if client.get(f"{self.url}/api/status").status_code == 200:
                        return
                except httpx.HTTPError:
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
        client = httpx.Client(base_url=self.url, headers=ADMIN_HEADERS, trust_env=False, timeout=10)
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
