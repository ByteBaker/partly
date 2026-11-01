# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Elastic-2.0

"""Audit declared runtime and development dependencies without building the SDK."""

import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def main() -> None:
    project = tomllib.loads((ROOT / "sdk/python/pyproject.toml").read_text())["project"]
    requirements = (ROOT / "config/requirements-dev.txt").read_text()
    requirements += "\n" + "\n".join([
        *project["dependencies"], *project["optional-dependencies"]["openfeature"],
    ]) + "\n"
    # Resolve the declared requirements, not an installed unpublished Partly wheel.
    with tempfile.TemporaryDirectory(prefix="partly-audit-") as directory:
        path = Path(directory) / "requirements.txt"
        path.write_text(requirements)
        subprocess.run([
            sys.executable, "-m", "pip_audit", "--strict", "--progress-spinner", "off",
            "-r", str(path),
        ], check=True)


if __name__ == "__main__":
    main()
