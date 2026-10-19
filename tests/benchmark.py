# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Elastic-2.0

"""Small local baseline, not a load test. Run with the release binary and wheel."""
import asyncio
import json
import os
import platform
import signal
import statistics
import subprocess
import tempfile
import time
import timeit
from contextlib import contextmanager
from pathlib import Path

import httpx

from partly import Client

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(os.environ.get("PARTLY_BINARY", ROOT / "target/release/partly")).resolve()


@contextmanager
def running_server():
    with tempfile.TemporaryDirectory(prefix="partly-bench-") as directory:
        process = subprocess.Popen(
            [str(BINARY), "serve", "--port", "0", "--db", str(Path(directory) / "partly.db")],
            cwd=directory, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        try:
            line = process.stdout.readline().strip()
            if "listening at " not in line:
                raise RuntimeError("Benchmark server failed to start")
            yield process, line.split("listening at ", 1)[1]
        finally:
            if process.poll() is None:
                process.send_signal(signal.SIGINT)
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
            process.stdout.close()
            process.stderr.close()


async def main():
    startups = []
    with httpx.Client(trust_env=False, timeout=5, headers={"X-Partly-Request": "dashboard"}) as admin:
        for _ in range(5):
            start = time.perf_counter()
            with running_server() as (_, url):
                response = admin.get(f"{url}/api/status")
                response.raise_for_status()
                startups.append((time.perf_counter() - start) * 1000)
        with running_server() as (process, url):
            admin.post(f"{url}/api/setup", json={"username": "benchmark", "password": "temporary-benchmark-password"}).raise_for_status()
            admin.post(f"{url}/api/environments/development/flags", json={"key": "checkout", "mode": "percentage", "percentage": 50}).raise_for_status()
            response = admin.post(f"{url}/api/environments/development/keys", json={"name": "Benchmark"})
            response.raise_for_status()
            key = response.json()["secret"]
            client = Client.builder().url(url).secret_key(key).build()
            try:
                await client.init()
                samples = timeit.repeat(lambda: client.evaluate("checkout", context={"id": "user-123"}), number=100_000, repeat=5)
                rss_kib = int(subprocess.check_output(["ps", "-o", "rss=", "-p", str(process.pid)], text=True).strip())
            finally:
                await client.aclose()
    print(json.dumps({
        "platform": platform.platform(),
        "python": platform.python_version(),
        "binary_bytes": BINARY.stat().st_size,
        "startup_median_ms": round(statistics.median(startups), 2),
        "server_rss_after_setup_mib": round(rss_kib / 1024, 2),
        "python_evaluate_median_us": round(statistics.median(samples) / 100_000 * 1_000_000, 3),
        "method": "5 startup samples to first status response; 5 batches of 100,000 Python evaluate calls; one 50% flag, fixed user ID; macOS/Linux ps RSS; no application load",
    }, indent=2))


if __name__ == "__main__":
    asyncio.run(main())
