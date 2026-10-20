# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Elastic-2.0

import json
import subprocess

import pytest
from conftest import ROOT, make_key, save_flag


@pytest.mark.parametrize("language", ["rust", "go", "node"])
def test_language_quickstart(language, server, admin, tmp_path):
    key_id, key = make_key(admin)
    filename = {"rust": "quickstart.rs", "go": "main.go", "node": "quickstart.js"}[language]
    source = (ROOT / "examples" / language / filename).read_text()
    source = source.replace('"http://127.0.0.1:8080"', json.dumps(server.url))
    source = source.replace('"YOUR_API_KEY"', json.dumps(key))

    if language == "rust":
        (tmp_path / "Cargo.toml").write_text(f"""
[package]
name = "partly-quickstart-check"
version = "0.0.0"
edition = "2021"
[workspace]
[[bin]]
name = "quickstart"
path = "quickstart.rs"
[dependencies]
partly = {{ package = "partly-rust", path = {json.dumps(str(ROOT / "sdk/rust"))} }}
tokio = {{ version = "1", features = ["macros", "rt-multi-thread"] }}
""")
        target = ROOT / "target/sdk-examples"
        build = ["cargo", "build", "--offline", "--target-dir", str(target)]
        command = [str(target / "debug/quickstart")]
    elif language == "go":
        subprocess.run(["sh", str(ROOT / "sdk/go/build.sh")],
                       cwd=ROOT, check=True, capture_output=True, timeout=120)
        (tmp_path / "go.mod").write_text((ROOT / "examples/go/go.mod").read_text().replace(
            "../../sdk/go", json.dumps(str(ROOT / "sdk/go"))))
        command = [str(tmp_path / "quickstart")]
        build = ["go", "build", "-a", "-o", command[0], "."]
    else:
        source = source.replace('"../../sdk/node"', json.dumps(str(ROOT / "sdk/node")))
        build = ["node", str(ROOT / "sdk/node/build.js")]
        command = ["node", str(tmp_path / filename)]
    (tmp_path / filename).write_text(source)
    result = subprocess.run(build, cwd=tmp_path, capture_output=True, text=True,
                            timeout=300 if language == "go" else 120)
    assert result.returncode == 0, result.stdout + result.stderr

    for enabled, expected in [(True, "Use the new checkout"), (False, "Use the current checkout")]:
        save_flag(admin, enabled=enabled)
        result = subprocess.run(command, cwd=tmp_path, capture_output=True, text=True, timeout=10)
        assert result.returncode == 0, result.stderr
        assert result.stdout.splitlines() == [expected, expected]
        assert key not in result.stdout + result.stderr
    assert admin.delete(f"/api/keys/{key_id}").status_code == 204
    result = subprocess.run(command, cwd=tmp_path, capture_output=True, text=True, timeout=10)
    assert result.returncode != 0
    assert not result.stdout
    assert key not in result.stderr
