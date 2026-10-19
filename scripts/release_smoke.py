"""Check installed release artifacts; needs the wheel, trustme, and packaging."""

import argparse
import asyncio
import importlib.metadata
import shutil
import signal
import socket
import ssl
import subprocess
import sys
import tempfile
import time
from pathlib import Path

import httpx
import trustme
from packaging.version import Version

import partly
from partly import Client, Decision


async def exercise(url, bundle, verify):
    async with httpx.AsyncClient(
        base_url=url, verify=verify, trust_env=False, timeout=10,
        headers={"X-Partly-Request": "dashboard", "Origin": url},
    ) as admin:
        response = await admin.get("/")
        assert response.status_code == 200 and "Partly" in response.text
        response = await admin.post("/api/setup", json={
            "username": "release-smoke", "password": "release-smoke-password",
        })
        assert response.status_code == 200
        assert "; Secure" in response.headers["set-cookie"]
        response = await admin.post("/api/environments/development/flags", json={
            "key": "release_smoke", "mode": "enabled",
        })
        assert response.status_code == 201
        response = await admin.post("/api/environments/development/keys", json={"name": "Release smoke"})
        assert response.status_code == 200
        client = Client.builder().url(url).secret_key(response.json()["secret"]).ca_bundle(bundle).build()
        try:
            await client.init()
            assert client.evaluate("release_smoke", context={}) is Decision.ENABLED
            assert await client.refresh() is False
            response = await admin.put("/api/environments/development/flags/release_smoke", json={"mode": "disabled"})
            assert response.status_code == 200
            assert await client.refresh() is True
            assert client.evaluate("release_smoke", context={}) is Decision.DISABLED
        finally:
            await client.aclose()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path, help="Built Partly executable")
    args = parser.parse_args()
    assert Path(partly.__file__).resolve().is_relative_to(Path(sys.prefix).resolve()), "SDK must be installed in this environment"
    sdk_version = Version(importlib.metadata.version("partly-sdk"))
    with tempfile.TemporaryDirectory(prefix="partly-release-") as directory:
        work = Path(directory)
        binary = work / "partly"
        shutil.copy2(args.binary.resolve(), binary)
        output = subprocess.check_output([str(binary), "--version"], text=True, cwd=work).strip()
        assert output.startswith("partly ") and Version(output.split()[1]) == sdk_version
        ca = trustme.CA()
        certificate = ca.issue_cert("127.0.0.1")
        bundle, cert, key = (work / name for name in ("ca.pem", "server.pem", "server-key.pem"))
        ca.cert_pem.write_to_path(bundle)
        cert.write_bytes(b"".join(pem.bytes() for pem in certificate.cert_chain_pems))
        certificate.private_key_pem.write_to_path(key)
        verify = ssl.create_default_context(cafile=str(bundle))
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            port = sock.getsockname()[1]
        url = f"https://127.0.0.1:{port}"
        with (work / "server.log").open("w+") as log:
            process = subprocess.Popen([
                str(binary), "serve", "--port", str(port), "--db", str(work / "partly.db"),
                "--tls-cert", str(cert), "--tls-key", str(key),
            ], cwd=work, stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 15
                with httpx.Client(verify=verify, trust_env=False, timeout=0.5) as probe:
                    while time.monotonic() < deadline:
                        if process.poll() is not None:
                            log.seek(0)
                            raise AssertionError(f"Server exited during startup: {log.read()}")
                        try:
                            response = probe.get(f"{url}/api/status")
                            if response.status_code == 200:
                                assert response.json()["setup_required"]
                                break
                        except httpx.HTTPError:
                            pass
                        time.sleep(0.05)
                    else:
                        raise AssertionError("Packaged server did not become ready")
                assert not (work / "partly.db").exists()
                asyncio.run(exercise(url, bundle, verify))
            finally:
                if process.poll() is None:
                    process.send_signal(signal.SIGTERM)
                    try:
                        process.wait(timeout=8)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
                        raise AssertionError("Server did not stop after SIGTERM") from None
            assert process.returncode == 0, "Server did not shut down cleanly"
    print(f"Release smoke passed: Partly {sdk_version} on {sys.platform}")


if __name__ == "__main__":
    main()
