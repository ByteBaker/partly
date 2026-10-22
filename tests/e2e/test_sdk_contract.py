# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Elastic-2.0

"""One real-server contract, exercised through each language's public SDK."""

import json
import os
import select
import shutil
import subprocess
import sys
import tarfile
import threading
import time
from contextlib import closing
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import pytest
import trustme
from conftest import ROOT, RULE_CASES, Server, make_key, save_flag, scope_for

DRIVERS = ROOT / "tests/sdk_contract"


class Driver:
    def __init__(self, command, directory, *, cwd=ROOT):
        self.log_path = directory / "driver.log"
        self.log = self.log_path.open("wb")
        self.process = subprocess.Popen(
            command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.log,
            cwd=cwd, bufsize=0,
        )

    def fail(self, reason):
        raise AssertionError(f"{reason}\n{self.log_path.read_text(errors='replace')}")

    def call(self, op, *, deadline_seconds=5, **arguments):
        if op == "build":
            environment, application = scope_for(arguments["key"])
            arguments.setdefault("environment", environment)
            arguments.setdefault("application_id", application)
        try:
            self.process.stdin.write(json.dumps({"op": op, **arguments}).encode() + b"\n")
            self.process.stdin.flush()
        except BrokenPipeError:
            self.fail(f"SDK driver exited during {op}")
        deadline = time.monotonic() + deadline_seconds
        response = bytearray()
        while b"\n" not in response:
            remaining = deadline - time.monotonic()
            if remaining <= 0 or not select.select([self.process.stdout], [], [], remaining)[0]:
                self.fail(f"SDK driver timed out during {op}")
            chunk = os.read(self.process.stdout.fileno(), 4096)
            if not chunk:
                self.fail(f"SDK driver exited during {op}")
            response.extend(chunk)
            if len(response) > 65536:
                self.fail(f"SDK driver response exceeded 64 KiB during {op}")
        try:
            return json.loads(response)
        except ValueError:
            self.fail(f"SDK driver returned invalid JSON during {op}")

    def finish(self):
        self.process.stdin.close()
        try:
            self.process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.fail("SDK driver did not shut down after EOF")
        if self.process.returncode != 0:
            self.fail(f"SDK driver exited with status {self.process.returncode}")

    def stop(self):
        if self.process.poll() is None:
            self.process.kill()
        self.process.wait()
        self.process.stdin.close()
        self.process.stdout.close()
        self.log.close()


def unpack_package(pattern, directory, packages):
    archives = list(packages.glob(pattern))
    assert len(archives) == 1, f"Expected one {pattern} in {packages}"
    with tarfile.open(archives[0]) as archive:
        root = archive.getnames()[0].split("/")[0]
        archive.extractall(directory, filter="data")
    return directory / root


def install_node_package(directory, packages, *, openfeature=False):
    archives = list(packages.glob("partly-node-*.tgz"))
    assert len(archives) == 1, f"Expected one Node package in {packages}"
    dependencies = [str(archives[0])]
    if openfeature:
        version = json.loads((ROOT / "package.json").read_text())["devDependencies"]["@openfeature/server-sdk"]
        dependencies.append(f"@openfeature/server-sdk@{version}")
    # A fresh CI cache may lack the optional peer's registry metadata after npm ci.
    subprocess.run(["npm", "install", *([] if openfeature else ["--offline"]), "--ignore-scripts", "--no-audit", "--no-fund",
                    "--prefix", str(directory), *dependencies],
                   cwd=directory, check=True, stdout=subprocess.DEVNULL, timeout=120)


def packaged_driver(language, directory, packages, *, openfeature=False):
    """Use only extracted packages and copied drivers in a fresh application directory."""
    def check_notices(package, *, compiled=False):
        assert "Apache License" in (package / "LICENSE").read_text()
        names = ["NOTICE", "THIRD_PARTY_NOTICES.txt"]
        if compiled:
            names.append("RUST_STDLIB_NOTICES.html")
        for name in names:
            assert (package / name).stat().st_size > 0, f"Missing packaged notice: {name}"

    if language == "python":
        entry = directory / "driver.py"
        shutil.copy2(DRIVERS / "python/driver.py", entry)
        return [sys.executable, "-I", str(entry)]
    if language == "rust":
        core = unpack_package("partly-core-*.crate", directory, packages)
        sdk = unpack_package("partly-rust-*.crate", directory, packages)
        assert "Apache License" in (core / "LICENSE").read_text()
        assert (core / "NOTICE").stat().st_size > 0
        check_notices(sdk)
        application = directory / "rust"
        shutil.copytree(DRIVERS / "rust", application)
        manifest = application / "Cargo.toml"
        content = manifest.read_text()
        if openfeature:
            shutil.copy2(application / "openfeature.rs", application / "src/main.rs")
            content = content.replace('path = "../../../sdk/rust"', 'path = "../../../sdk/rust", features = ["openfeature"]')
            content += '\nopen-feature = "0.3.0"\n'
        manifest.write_text(content.replace(
            'path = "../../../sdk/rust"', f"path = {json.dumps(str(sdk))}",
        ) + f'\n[patch.crates-io]\npartly-core = {{ path = {json.dumps(str(core))} }}\n')
        shutil.copy2(sdk / "Cargo.lock", application / "Cargo.lock")
        target = directory.parent / "packaged-rust-target"
        subprocess.run(["cargo", "build", "--offline", "--target-dir", str(target)], cwd=application, check=True,
                       stdout=subprocess.DEVNULL, timeout=300)
        return [str(target / "debug/partly-sdk-contract")]
    if language == "go":
        sdk = unpack_package("partly-go-*.tar.gz", directory, packages)
        check_notices(sdk, compiled=True)
        application = directory / "go"
        shutil.copytree(DRIVERS / ("go_openfeature" if openfeature else "go"), application)
        manifest = application / "go.mod"
        manifest.write_text(manifest.read_text()
                            .replace("../../../sdk/go/openfeature", json.dumps(str(sdk / "openfeature")))
                            .replace("../../../sdk/go", json.dumps(str(sdk))))
        entry = application / "driver"
        subprocess.run(["go", "build", "-a", "-o", str(entry), "."], cwd=application,
                       check=True, stdout=subprocess.DEVNULL, timeout=300)
        return [str(entry)]
    install_node_package(directory, packages, openfeature=openfeature)
    check_notices(directory / "node_modules/partly-sdk", compiled=True)
    entry = directory / "driver.mjs"
    entry.write_text((DRIVERS / ("node/openfeature.mjs" if openfeature else "node/driver.mjs"))
                     .read_text().replace("../../../sdk/node", "partly-sdk"))
    return ["node", str(entry)]


@pytest.fixture
def driver(language, tmp_path):
    tool = {"python": sys.executable, "rust": "cargo", "go": "go", "node": "node"}[language]
    if not shutil.which(tool):
        pytest.fail(f"{language}: required tool not found: {tool}", pytrace=False)
    packages = os.environ.get("PARTLY_SDK_PACKAGES")
    if packages:
        command = packaged_driver(language, tmp_path, Path(packages).resolve())
    elif language in ("rust", "go"):
        if language == "rust":
            target = ROOT / "target"
            build = ["cargo", "build", "--locked", "--offline", "--manifest-path", str(DRIVERS / "rust/Cargo.toml"),
                     "--target-dir", str(target)]
            command = [str(target / "debug/partly-sdk-contract")]
        else:
            subprocess.run(["sh", str(ROOT / "sdk/go/build.sh")],
                           cwd=ROOT, check=True, capture_output=True, timeout=120)
            command = [str(tmp_path / "go-driver")]
            build = ["go", "build", "-a", "-o", command[0], "."]
        result = subprocess.run(build, cwd=DRIVERS / language, capture_output=True, text=True,
                                timeout=300 if language == "go" else 120)
        if result.returncode:
            pytest.fail(f"{language} SDK driver cannot build:\n{result.stdout}{result.stderr}", pytrace=False)
    elif language == "python":
        command = [sys.executable, "-I", str(DRIVERS / "python/driver.py")]
    else:
        subprocess.run(["node", str(ROOT / "sdk/node/build.js")],
                       cwd=ROOT, check=True, capture_output=True, timeout=120)
        command = ["node", str(DRIVERS / "node/driver.mjs")]
    instance = Driver(command, tmp_path, cwd=tmp_path if packages else ROOT)
    try:
        yield instance
    finally:
        instance.stop()


@pytest.mark.parametrize("language", ["python", "rust", "go", "node"])
def test_sdk_contract(language, driver, tmp_path):
    ca = trustme.CA()
    certificate = ca.issue_cert("127.0.0.1")
    bundle, cert, key = (tmp_path / name for name in ("ca.pem", "server.pem", "server-key.pem"))
    ca.cert_pem.write_to_path(bundle)
    cert.write_bytes(b"".join(pem.bytes() for pem in certificate.cert_chain_pems))
    certificate.private_key_pem.write_to_path(key)
    server = Server(tmp_path, tls_cert=cert, tls_key=key, ca_bundle=bundle)

    def build(secret, *, trusted=True, interval=3600):
        assert driver.call(
            "build", url=server.url, key=secret, interval=interval, timeout=1,
            ca_bundle=str(bundle) if trusted else None,
        ) == {"ok": True}

    def evaluate(flag, context=None):
        return driver.call("evaluate", flag=flag, context=context or {})

    def type_error(flag, key, op, expected, actual):
        return {"error": "context_type", "flag_key": flag, "k": key, "op": op,
                "expected_type": expected, "actual_type": actual}

    try:
        # An unreachable control plane cannot prevent construction.
        build("invalid-key")
        assert evaluate("always") == {"error": "policy_not_ready"}
        assert driver.call("init") == {"error": "refresh"}
        server.start()
        with closing(server.admin()) as admin:
            assert driver.call("init") == {"error": "authentication"}
            assert driver.call("close") == {"ok": True}
            key_id, secret = make_key(admin)
            save_flag(admin, key="always", flag={"mode": "enabled"})
            save_flag(admin, key="never", flag={"mode": "disabled"})
            save_flag(admin, key="zero", percentage=0)
            save_flag(admin, key="checkout", percentage=50)
            save_flag(admin, key="new_checkout", percentage=50)
            for op, value, *_ in RULE_CASES:
                save_flag(admin, key=f"rule_{op}", flag={
                    "mode": "custom", "action": "enable",
                    "groups": [[{"k": "value", "op": op, "v": value}]],
                })
            groups = [
                [{"k": "plan", "op": "matches", "v": "pro"}, {"k": "age", "op": "ge", "v": 18}],
                [{"k": "beta", "op": "is", "v": True}],
            ]
            save_flag(admin, key="groups", flag={"mode": "custom", "action": "enable", "groups": groups})
            numeric_rules = {
                "large_integer": ("eq", 9007199254740993),
                "unsigned_max": ("eq", 18446744073709551615),
                "signed_min": ("eq", -9223372036854775808),
                "rounded_float": ("eq", 1000000000000000100),
                "fraction": ("gt", 1.20000003),
            }
            for name, (op, value) in numeric_rules.items():
                save_flag(admin, key=name, flag={
                    "mode": "custom", "action": "enable",
                    "groups": [[{"k": "value", "op": op, "v": value}]],
                })

            build(secret, trusted=False)
            assert driver.call("init") == {"error": "refresh"}
            assert driver.call("close") == {"ok": True}
            build(secret)
            assert driver.call("init") == {"ok": True}
            assert driver.call("init") == {"ok": True}
            assert evaluate("always") == {"decision": "enabled"}
            assert evaluate("never") == {"decision": "disabled"}
            assert evaluate("zero", {"id": "user-42"}) == {"decision": "disabled"}
            assert evaluate("undefined") == {"error": "flag_not_found"}
            for value, actual in [(None, "null"), ([], "array"), ({"nested": True}, "object")]:
                assert evaluate("always", {"extra": value}) == type_error(
                    "always", "extra", None, "string, number, or boolean", actual,
                )
                assert evaluate("rule_ge", {"value": value}) == type_error(
                    "rule_ge", "value", "ge", "number", actual,
                )
                assert evaluate("undefined", {"value": value}) == {"error": "flag_not_found"}
            assert evaluate("undefined", {"value": 2**65}) == {"error": "flag_not_found"}
            for flag, user, expected in [
                ("checkout", "user-42", "disabled"),
                ("new_checkout", "user-123", "enabled"),
                ("checkout", "用户-1", "disabled"),
            ]:
                assert evaluate(flag, {"id": user}) == {"decision": expected}
            assert evaluate("checkout") == {"error": "invalid_context"}
            assert evaluate("checkout", {"id": ""}) == {"error": "invalid_context"}
            assert evaluate("checkout", {"id": 42}) == type_error(
                "checkout", "id", None, "string", "number",
            )
            selected = {user: evaluate("checkout", {"id": f"user-{user}"}) for user in range(10)}
            expanded = {user: evaluate("checkout", {"id": f"user-{user}"}) for user in range(100)}
            assert selected == {user: expanded[user] for user in selected}

            for op, _, matched, unmatched, wrong, expected, actual in RULE_CASES:
                assert evaluate(f"rule_{op}", {"value": matched}) == {"decision": "enabled"}
                assert evaluate(f"rule_{op}", {"value": unmatched}) == {"decision": "disabled"}
                assert evaluate(f"rule_{op}", {"value": wrong}) == type_error(
                    f"rule_{op}", "value", op, expected, actual,
                )
                assert evaluate(f"rule_{op}") == {"error": "invalid_context"}
            for flag, value, decision in [
                ("large_integer", 9007199254740993, "enabled"),
                ("large_integer", 9007199254740992, "disabled"),
                ("unsigned_max", 18446744073709551615, "enabled"),
                ("unsigned_max", 18446744073709551614, "disabled"),
                ("signed_min", -9223372036854775808, "enabled"),
                ("signed_min", -9223372036854775807, "disabled"),
                ("rounded_float", float(1000000000000000100), "disabled"),
                ("rule_eq", 1e20, "disabled"),
                ("fraction", 1.2000000476837158, "enabled"),
                ("fraction", 1.2, "disabled"),
            ]:
                # Node rejects unsafe integer-valued numbers instead of rounding them.
                expected = ({"error": "invalid_context"} if language == "node" and abs(value) > 2**53 - 1
                            else {"decision": decision})
                assert evaluate(flag, {"value": value}) == expected
            member = {"plan": "pro", "age": 21, "beta": False}
            outsider = {"plan": "free", "age": 16, "beta": False}
            assert evaluate("groups", member) == {"decision": "enabled"}
            assert evaluate("groups", outsider) == {"decision": "disabled"}
            assert evaluate("groups", {**outsider, "beta": True}) == {"decision": "enabled"}
            assert evaluate("groups", {"beta": True}) == {"error": "invalid_context"}
            assert evaluate("groups", {**member, "age": "21", "beta": True}) == type_error(
                "groups", "age", "ge", "number", "string",
            )
            assert driver.call("refresh") == {"updated": False}

            save_flag(admin, key="always", environment="production", flag={"mode": "disabled"})
            assert driver.call("refresh") == {"updated": False}
            save_flag(admin, key="always", flag={"mode": "disabled"})
            save_flag(admin, key="checkout", percentage=100)
            save_flag(admin, key="groups", flag={"mode": "custom", "action": "disable", "groups": groups})
            assert evaluate("always") == {"decision": "enabled"}
            assert driver.call("refresh") == {"updated": True}
            assert evaluate("always") == {"decision": "disabled"}
            assert evaluate("groups", member) == {"decision": "disabled"}
            assert evaluate("groups", outsider) == {"decision": "enabled"}
            assert all(evaluate("checkout", {"id": f"user-{user}"}) == {"decision": "enabled"} for user in expanded)
            assert driver.call("refresh") == {"updated": False}

            assert driver.call("close") == {"ok": True}
            build(secret, interval=0.05)
            assert driver.call("init") == {"ok": True}
            save_flag(admin, key="always", flag={"mode": "enabled"})
            deadline = time.monotonic() + 5
            while evaluate("always") != {"decision": "enabled"}:
                assert time.monotonic() < deadline, "Background polling did not update the policy"
                time.sleep(0.02)
            assert admin.delete(f"/api/keys/{key_id}").status_code == 204
            assert driver.call("refresh") == {"error": "key_revoked"}
            assert evaluate("always") == {"error": "key_revoked"}
            assert driver.call("init") == {"error": "key_revoked"}
            assert driver.call("close") == {"ok": True}
            assert evaluate("always") == {"error": "key_revoked"}
            _, secret = make_key(admin)
            build(secret)
            assert driver.call("init") == {"ok": True}
            server.stop()
            assert driver.call("refresh") == {"error": "refresh"}
            assert evaluate("always") == {"decision": "enabled"}
            assert driver.call("close") == {"ok": True}
            assert driver.call("close") == {"ok": True}
            assert evaluate("always") == {"decision": "enabled"}
            assert driver.call("init") == {"error": "client_closed"}
            assert driver.call("refresh") == {"error": "client_closed"}
            build(secret)
            assert driver.call("init") == {"error": "refresh"}
            assert evaluate("always") == {"error": "policy_not_ready"}
            assert driver.call("close") == {"ok": True}
            driver.finish()
            assert secret not in driver.log_path.read_text()
            assert secret not in (tmp_path / "server.log").read_text()
    finally:
        server.stop()


@pytest.mark.parametrize("language", ["python", "rust", "go", "node"])
def test_sdk_rejects_bad_refresh_without_losing_policy_or_etag(language, driver):
    policy = {"schema_version": 1, "application_id": "app_test", "project": "default", "environment": "development",
              "revision": 1, "flags": {"flag": {"mode": "enabled"}}}
    response = [304, b"", {"ETag": '"one"'}, 0]
    requests = []

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def do_GET(self):
            requests.append((self.path, self.headers.get("If-None-Match")))
            status, body, headers, delay = response
            self.send_response(status)
            for key, value in headers.items():
                self.send_header(key, value)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            time.sleep(delay)
            try:
                self.wfile.write(body)
            except (BrokenPipeError, ConnectionResetError):
                pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        assert driver.call("build", url=f"http://127.0.0.1:{server.server_port}",
                           key="test-secret", interval=3600, timeout=0.2) == {"ok": True}
        assert driver.call("init") == {"error": "invalid_policy"}
        response[:] = [200, json.dumps({**policy, "application_id": "app_wrong"}).encode(), {"ETag": '"one"'}, 0]
        assert driver.call("init") == {"error": "invalid_policy"}
        response[:] = [200, json.dumps(policy).encode(), {"ETag": '"one"'}, 0]
        assert driver.call("init") == {"ok": True}
        invalid_bodies = [
            b"not json", b"[]", json.dumps({**policy, "schema_version": 2}).encode(),
            json.dumps({**policy, "environment": "production"}).encode(),
            json.dumps({**policy, "application_id": "app_other"}).encode(),
            json.dumps({**policy, "unexpected": True}).encode(),
            json.dumps({**policy, "flags": {"flag": {"mode": "percentage", "percentage": 101}}}).encode(),
            json.dumps({**policy, "flags": {"flag": {"mode": "custom", "action": "enable",
                "groups": [[{"k": "name", "op": "regex", "v": "(?i)pro"}]]}}}).encode(),
            b" " * (1024 * 1024 + 1),
        ]
        bad_responses = [[200, body, {"ETag": '"two"'}, 0, "invalid_policy"] for body in invalid_bodies]
        bad_responses.extend([
            [200, json.dumps(policy).encode(), {}, 0, "invalid_policy"],
            [200, json.dumps(policy).encode(), {"ETag": 'W/"two"'}, 0, "invalid_policy"],
            [304, b"", {"ETag": '"wrong"'}, 0, "invalid_policy"],
            [302, b"", {"Location": f"http://127.0.0.1:{server.server_port}/leaked"}, 0, "refresh"],
            [403, b"secret response body", {}, 0, "authentication"],
            [200, json.dumps(policy).encode(), {"ETag": '"two"'}, 0.5, "refresh"],
        ])
        for status, body, headers, delay, error in bad_responses:
            response[:] = [status, body, headers, delay]
            assert driver.call("refresh") == {"error": error}
            assert requests[-1] == ("/api/policy", '"one"')
            assert driver.call("evaluate", flag="flag", context={}) == {"decision": "enabled"}
        response[:] = [200, json.dumps({**policy, "flags": {"flag": {"mode": "disabled"}}}).encode(),
                       {"ETag": '"two"'}, 0]
        assert driver.call("refresh") == {"updated": True}
        assert driver.call("evaluate", flag="flag", context={}) == {"decision": "disabled"}
        response[:] = [304, b"", {"ETag": '"two"'}, 0]
        assert driver.call("refresh") == {"updated": False}
        assert requests[-1] == ("/api/policy", '"two"')
        assert all(path == "/api/policy" for path, _ in requests)
        assert driver.call("close") == {"ok": True}
        driver.finish()
    finally:
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)


@pytest.mark.parametrize("program, message", [
    ("import sys; sys.stdin.readline(); sys.stderr.write('import failed\\n'); sys.exit(1)", "import failed"),
    ("import sys; sys.stdin.readline(); print('not json', flush=True)", "invalid JSON"),
    ("import sys, time; sys.stdin.readline(); print('{', end='', flush=True); time.sleep(10)", "timed out"),
])
def test_contract_driver_reports_failures(tmp_path, program, message):
    driver = Driver([sys.executable, "-I", "-c", program], tmp_path)
    try:
        with pytest.raises(AssertionError, match=message):
            driver.call("init", deadline_seconds=1)
    finally:
        driver.stop()


@pytest.mark.parametrize("language", ["python", "rust", "go", "node"])
def test_application_scope_selection_and_background_revocation(language, driver, server, admin):
    from test_applications import create_app

    for flag in ("shared", "private"):
        assert admin.post("/api/environments/development/flags", json={"key": flag, "mode": "enabled"}).status_code == 201
    app = create_app(admin, "Storefront", ["shared"])
    other = create_app(admin, "Billing", ["private"])
    key = app["keys"][0]

    def build(application_id, interval=3600):
        assert driver.call("build", url=server.url, key=key["secret"], environment="development",
                           application_id=application_id, interval=interval, timeout=1) == {"ok": True}

    build(other["id"])
    assert driver.call("init") == {"error": "authentication"}
    assert driver.call("close") == {"ok": True}
    build(app["id"])
    assert driver.call("init") == {"ok": True}
    assert driver.call("evaluate", flag="shared", context={}) == {"decision": "enabled"}
    assert driver.call("evaluate", flag="private", context={}) == {"error": "flag_not_found"}
    assert admin.put(f"/api/environments/development/apps/{app['id']}", json={"name": "Storefront", "flags": []}).status_code == 200
    assert driver.call("refresh") == {"updated": True}
    assert driver.call("evaluate", flag="shared", context={}) == {"error": "flag_not_found"}
    assert driver.call("refresh") == {"updated": False}
    assert driver.call("close") == {"ok": True}
    build(app["id"], interval=0.02)
    assert driver.call("init") == {"ok": True}
    assert admin.delete(f"/api/keys/{key['id']}").status_code == 204
    deadline = time.monotonic() + 5
    while driver.call("evaluate", flag="shared", context={}) != {"error": "key_revoked"}:
        assert time.monotonic() < deadline, "Background refresh did not clear the revoked policy"
        time.sleep(0.02)
    assert driver.call("refresh") == {"error": "key_revoked"}
    assert driver.call("init") == {"error": "key_revoked"}
    assert driver.call("close") == {"ok": True}
    assert driver.call("evaluate", flag="shared", context={}) == {"error": "key_revoked"}
