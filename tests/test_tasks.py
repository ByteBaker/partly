# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Elastic-2.0

"""Task commands must fail safely and run from the checkout root."""

import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

from scripts import audit_python

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


@pytest.mark.parametrize(("stage", "failed"), [
    ("manual", ""), ("pre-push", ""), ("pre-push", "rust"),
    ("pre-push", "python"), ("pre-push", "go"), ("pre-push", "node"),
])
def test_prepush_audits_run_for_docs_and_block_scanner_failures(checkout, stage, failed):
    root, env = checkout
    subprocess.run(["git", "init", "--quiet", str(root)], check=True)
    (root / "config").mkdir()
    shutil.copy2(ROOT / "config/prek.toml", root / "config/prek.toml")
    (root / "README.md").write_text("Documentation-only change.\n")
    task = Path(env["PATH"].split(os.pathsep)[0]) / "task"
    task.write_text(
        '#!/bin/sh\nprintf "%s\\n" "$1" >> "$TASK_TRACE"\n'
        'if [ "$1" = "$AUDIT_FAIL" ]; then exit 19; fi\n'
    )
    task.chmod(0o755)
    prek = Path(sys.executable).parent / "prek"
    result = subprocess.run([
        str(prek), "run", "-c", "config/prek.toml", "--stage", stage, "--files", "README.md",
    ], cwd=root, env={**env, "PREK_HOME": str(root / ".prek-cache"), "AUDIT_FAIL": f"audit:{failed}"})
    assert (result.returncode == 0) is (not failed)
    if stage == "manual":
        assert not Path(env["TASK_TRACE"]).exists()
    else:
        assert set(Path(env["TASK_TRACE"]).read_text().splitlines()) == {
            "audit:rust", "audit:python", "audit:go", "audit:node",
        }


def test_python_audit_covers_declared_dependencies_and_propagates_failure(tmp_path, monkeypatch):
    (tmp_path / "sdk/python").mkdir(parents=True)
    (tmp_path / "config").mkdir()
    (tmp_path / "sdk/python/pyproject.toml").write_text(
        '[project]\ndependencies = ["httpx>=0.28,<0.29"]\n'
        '[project.optional-dependencies]\nopenfeature = ["openfeature-sdk>=0.10,<0.11"]\n'
    )
    (tmp_path / "config/requirements-dev.txt").write_text("pytest==9.0.0\n")
    monkeypatch.setattr(audit_python, "ROOT", tmp_path)

    def fail(command, *, check):
        assert command[:3] == [sys.executable, "-m", "pip_audit"]
        assert "--strict" in command and check
        assert set(Path(command[-1]).read_text().split()) == {
            "pytest==9.0.0", "httpx>=0.28,<0.29", "openfeature-sdk>=0.10,<0.11",
        }
        raise subprocess.CalledProcessError(19, command)

    monkeypatch.setattr(audit_python.subprocess, "run", fail)
    with pytest.raises(subprocess.CalledProcessError) as failure:
        audit_python.main()
    assert failure.value.returncode == 19
    assert not Path(failure.value.cmd[-1]).exists()


@pytest.mark.parametrize(("contents", "finding"), [
    ("permissions: {contents: read}\n", None),
    ("permissions: write-all\n", "excessive-permissions"),
    ("permissions: write-all # zizmor: ignore[excessive-permissions]\n", "excessive-permissions"),
    ("permissions: [\n", "error"),
])
def test_workflow_security_blocks_unsafe_or_invalid_workflows(checkout, contents, finding):
    root, _ = checkout
    (root / ".github/workflows").mkdir(parents=True)
    (root / ".github/workflows/check.yml").write_text(
        "name: Checks\non: pull_request\n" + contents
        + "jobs:\n  check:\n    runs-on: ubuntu-latest\n"
        + "    steps:\n      - run: echo checked\n"
    )
    result = subprocess.run([
        "task", "--exit-code", "--dir", str(root), "lint:workflows", f"PYTHON={sys.executable}",
    ], text=True, capture_output=True)
    assert (result.returncode == 0) is (finding is None), result.stdout + result.stderr
    if finding:
        assert finding in (result.stdout + result.stderr).lower()


@pytest.mark.parametrize("exit_code", [0, 19])
def test_workflow_edits_run_security_hook_and_propagate_failure(checkout, exit_code):
    root, env = checkout
    subprocess.run(["git", "init", "--quiet", str(root)], check=True)
    (root / ".github/actions/setup").mkdir(parents=True)
    (root / "config").mkdir()
    shutil.copy2(ROOT / "config/prek.toml", root / "config/prek.toml")
    (root / ".github/actions/setup/action.yml").write_text("name: setup\n")
    task = Path(env["PATH"].split(os.pathsep)[0]) / "task"
    task.write_text(f'#!/bin/sh\nprintf "%s\\n" "$1" >> "$TASK_TRACE"\nexit {exit_code}\n')
    task.chmod(0o755)
    result = subprocess.run([
        str(Path(sys.executable).parent / "prek"), "run", "-c", "config/prek.toml",
        "workflow-security", "--stage", "manual", "--files", ".github/actions/setup/action.yml",
    ], cwd=root, env={**env, "PREK_HOME": str(root / ".prek-cache")})
    assert (result.returncode == 0) is (exit_code == 0)
    assert Path(env["TASK_TRACE"]).read_text().splitlines() == ["lint:workflows"]


@pytest.mark.parametrize("exit_code", [0, 19])
def test_go_audit_checks_each_module_and_propagates_failure(checkout, exit_code):
    root, env = checkout
    modules = ["sdk/go", "sdk/go/openfeature", "examples/go", "tests/sdk_contract/go", "tests/sdk_contract/go_openfeature"]
    for module in modules:
        (root / module).mkdir(parents=True, exist_ok=True)
    scanner = root / ".cache/audit-tools/bin/govulncheck"
    scanner.parent.mkdir(parents=True)
    scanner.write_text(
        '#!/bin/sh\nprintf "%s\\n" "$PWD" "$GOTOOLCHAIN" >> "$TASK_TRACE"\n'
        f'exit {exit_code}\n'
    )
    scanner.chmod(0o755)
    result = run_task(root, env, "audit:go")
    assert result.returncode == exit_code
    visited = Path(env["TASK_TRACE"]).read_text().splitlines()
    assert visited[::2] == [str(root / module) for module in (modules[:1] if exit_code else modules)]
    assert set(visited[1::2]) == {"go1.27.4"}
