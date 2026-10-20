# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Elastic-2.0

"""Keep one release version across SDKs and reject mislabeled archives."""

import json
import re
import tomllib
from pathlib import Path

import pytest

from scripts import package_sdks

ROOT = Path(__file__).resolve().parents[1]


def test_release_versions_match():
    workspace = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]
    version = workspace["package"]["version"]
    assert workspace["dependencies"]["partly-core"]["version"] == f"={version}"
    lock = tomllib.loads((ROOT / "Cargo.lock").read_text())["package"]
    locked = {(entry["name"], entry["version"]) for entry in lock}
    for directory in ("crates", "sdk"):
        for manifest in (ROOT / directory).glob("*/Cargo.toml"):
            package = tomllib.loads(manifest.read_text())["package"]
            assert package["version"] == {"workspace": True}, str(manifest)
            assert (package["name"], version) in locked, str(manifest)
    python = tomllib.loads((ROOT / "sdk/python/pyproject.toml").read_text())["project"]
    assert "version" in python["dynamic"] and "version" not in python
    assert json.loads((ROOT / "sdk/node/package.json").read_text())["version"] == version
    for name in ("examples/go/go.mod", "tests/sdk_contract/go/go.mod"):
        required = re.findall(r"^require partly\.sh/sdk/go (\S+)$", (ROOT / name).read_text(), re.MULTILINE)
        assert required == [f"v{version}"], name


def test_packaging_rejects_version_drift_before_building(tmp_path, monkeypatch):
    (tmp_path / "Cargo.toml").write_text('[workspace.package]\nversion = "0.1.0-alpha.2"\n')
    (tmp_path / "sdk/node").mkdir(parents=True)
    (tmp_path / "sdk/node/package.json").write_text('{"version": "0.1.0-alpha.1"}')
    monkeypatch.setattr(package_sdks, "ROOT", tmp_path)
    output = tmp_path / "packages"
    with pytest.raises(ValueError, match="version must match"):
        package_sdks.build(output)
    assert not output.exists()
