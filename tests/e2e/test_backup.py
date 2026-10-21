# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Elastic-2.0

import shutil
import sqlite3
from contextlib import closing

import httpx
import pytest
from conftest import Server, keys, make_key, policy_headers, save_flag


def test_stopped_database_backup_restores_flags_keys_and_admin(server, admin, tmp_path):
    _, key = make_key(admin, name="Retained application")
    save_flag(admin, percentage=25)
    server.stop()
    backup = tmp_path / "backup.db"
    shutil.copy2(server.db, backup)

    server.start()
    with closing(server.admin(setup=False)) as current:
        save_flag(current, percentage=75)
    server.stop()

    restored_directory = tmp_path / "restored"
    restored_directory.mkdir()
    restored = Server(restored_directory)
    shutil.copy2(backup, restored.db)
    try:
        restored.start()
        with closing(restored.admin(setup=False)) as recovered:
            flags = recovered.get("/api/environments/development/flags").json()
            assert flags["new_checkout"] == {"mode": "percentage", "percentage": 25}
            saved_keys = keys(recovered, "development")
            assert [(item["name"], item["secret"]) for item in saved_keys] == [("Retained application", key)]
        response = httpx.get(
            f"{restored.url}/api/policy", headers=policy_headers(key), trust_env=False,
        )
        assert response.status_code == 200
        assert response.json()["flags"]["new_checkout"]["percentage"] == 25
    finally:
        restored.stop()
    with sqlite3.connect(server.db) as database:
        assert database.execute("PRAGMA user_version").fetchone()[0] == 1


def test_future_database_version_is_rejected_without_modification(server, admin):
    server.stop()
    with sqlite3.connect(server.db) as database:
        database.execute("PRAGMA user_version = 2")
    original = server.db.read_bytes()
    with pytest.raises(RuntimeError, match="schema"):
        server.start()
    assert server.db.read_bytes() == original
