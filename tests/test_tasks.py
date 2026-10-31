# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Elastic-2.0

"""Task commands must fail safely and run from the checkout root."""

import os
import shutil
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]


@pytest.fixture
def checkout(tmp_path):
    root = tmp_path / "checkout with spaces"
    root.mkdir()
    shutil.copy2(ROOT / "Taskfile.yml", root)
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    for name, body in {
        "cargo": 'printf "%s\\n" "$PWD" "$@" >> "$TASK_TRACE"\nexit "${BUILD_EXIT:-0}"\n',
        "prek": 'printf "check\\n" >> "$TASK_TRACE"\nexit "${CHECK_EXIT:-0}"\n',
    }.items():
        tool = bin_dir / name
        tool.write_text("#!/bin/sh\n" + body)
        tool.chmod(0o755)
    env = {**os.environ, "PATH": f"{bin_dir}:{os.environ['PATH']}", "TASK_TRACE": str(tmp_path / "trace")}
    env.pop("PARTLY_PYTHON", None)
    return root, env


def run_task(root, env, *args, cwd=None):
    return subprocess.run(["task", "--exit-code", "--dir", str(root), *args], cwd=cwd, env=env)


def test_server_build_uses_checkout_root_without_setup(checkout, tmp_path):
    root, env = checkout
    result = run_task(root, env, "build:server", cwd=tmp_path)
    assert result.returncode == 0
    assert Path(env["TASK_TRACE"]).read_text().splitlines() == [
        str(root), "build", "--release", "--locked", "-p", "partly-server",
    ]


@pytest.mark.parametrize("task", ["build", "test"])
def test_failed_check_prevents_build_and_tests(checkout, task):
    root, env = checkout
    result = run_task(root, {**env, "CHECK_EXIT": "19"}, task)
    assert result.returncode == 19
    assert Path(env["TASK_TRACE"]).read_text() == "check\n"
    assert not (root / "target").exists()


def test_check_does_not_build(checkout):
    root, env = checkout
    assert run_task(root, env, "check").returncode == 0
    assert Path(env["TASK_TRACE"]).read_text() == "check\n"


def test_build_stops_on_first_compiler_failure(checkout):
    root, env = checkout
    result = run_task(root, {**env, "BUILD_EXIT": "17"}, "build")
    assert result.returncode == 17
    assert not (root / "target/wheels").exists()
    assert Path(env["TASK_TRACE"]).read_text().count("--locked") == 1


def test_unavailable_python_does_not_remove_previous_wheel(checkout):
    root, env = checkout
    wheel = root / "target/wheels/partly_sdk-existing.whl"
    wheel.parent.mkdir(parents=True)
    wheel.write_bytes(b"previous build")
    result = run_task(root, env, "build:python", f"PYTHON={root / 'missing-python'}")
    assert result.returncode != 0
    assert wheel.read_bytes() == b"previous build"


def test_setup_preserves_an_environment_that_fails_python_validation(checkout):
    root, env = checkout
    interpreter = root / ".venv/bin/python"
    interpreter.parent.mkdir(parents=True)
    original = '#!/bin/sh\nexit 23\n'
    interpreter.write_text(original)
    interpreter.chmod(0o755)
    result = run_task(root, env, "setup")
    assert result.returncode == 23
    assert interpreter.read_text() == original
    assert not Path(env["TASK_TRACE"]).exists()


def test_test_runner_stops_when_the_installed_sdk_check_fails(checkout, tmp_path):
    root, env = checkout
    interpreter = tmp_path / "python"
    interpreter.write_text('#!/bin/sh\nexit 23\n')
    interpreter.chmod(0o755)
    result = run_task(root, env, "test:run", f"PYTHON={interpreter}", cwd=tmp_path)
    assert result.returncode == 23
    assert not Path(env["TASK_TRACE"]).exists()


def test_gofmt_check_rejects_changes_and_syntax_errors_without_editing(tmp_path):
    source = tmp_path / "example.go"
    for contents, succeeds in [
        ("package example\n\nfunc example() {}\n", True),
        ("package example\nfunc example( ){ }\n", False),
        ("package example\nfunc {\n", False),
    ]:
        source.write_text(contents)
        result = subprocess.run(["sh", str(ROOT / "scripts/check_gofmt.sh"), str(source)])
        assert (result.returncode == 0) is succeeds
        assert source.read_text() == contents
