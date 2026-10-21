# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Elastic-2.0

import hashlib
import sqlite3
from contextlib import closing

import httpx


def create_app(admin, name, flags=(), env="development"):
    response = admin.post(f"/api/environments/{env}/apps", json={"name": name, "flags": list(flags)})
    assert response.status_code == 201, response.text
    return response.json()


def headers(app, env="development", key=None):
    return {"Authorization": f"Bearer {key or app['keys'][0]['secret']}",
            "X-Partly-Environment": env, "X-Partly-Application": app["id"]}


def test_app_selection_keys_and_audits(server, admin):
    for name in ("shared", "billing_only"):
        assert admin.post("/api/environments/development/flags", json={"key": name, "mode": "enabled"}).status_code == 201
    billing = create_app(admin, "Billing", ["shared", "billing_only"])
    shop = create_app(admin, "Storefront", ["shared"])
    empty = create_app(admin, "Empty")
    production = create_app(admin, "Billing", env="production")
    assert production["id"] != billing["id"]
    with httpx.Client(base_url=server.url, trust_env=False) as sdk:
        assert sdk.get("/api/policy", headers=headers(empty)).json()["flags"] == {}
        first = sdk.get("/api/policy", headers=headers(shop))
        assert list(first.json()["flags"]) == ["shared"]
        assert first.json()["application_id"] == shop["id"]
        assert set(sdk.get("/api/policy", headers=headers(billing)).json()["flags"]) == {"shared", "billing_only"}
        for wrong in [headers(shop, "production"), {**headers(shop), "X-Partly-Application": billing["id"]},
                      {"Authorization": headers(shop)["Authorization"]}]:
            assert sdk.get("/api/policy", headers=wrong).status_code == 401
        # An unrelated flag update must leave this application's ETag unchanged.
        assert admin.put("/api/environments/development/flags/billing_only", json={"mode": "disabled"}).status_code == 200
        assert sdk.get("/api/policy", headers={**headers(shop), "If-None-Match": first.headers["etag"]}).status_code == 304
        assert admin.put(f"/api/environments/development/apps/{shop['id']}", json={"name": "Storefront", "flags": []}).status_code == 200
        updated = sdk.get("/api/policy", headers={**headers(shop), "If-None-Match": first.headers["etag"]})
        assert updated.status_code == 200 and updated.json()["flags"] == {}
        replacement = admin.post(f"/api/environments/development/apps/{billing['id']}/keys", json={"name": "Replacement"}).json()
        old = billing["keys"][0]
        assert admin.delete(f"/api/keys/{old['id']}").status_code == 204
        revoked = sdk.get("/api/policy", headers={**headers(billing), "If-None-Match": "*"})
        assert revoked.status_code == 401 and revoked.json()["code"] == "key_revoked"
        assert revoked.headers["x-partly-error"] == "key_revoked"
        assert sdk.get("/api/policy", headers=headers(billing, key=replacement["secret"])).status_code == 200
    apps = admin.get("/api/environments/development/apps").json()["applications"]
    retained = next(app for app in apps if app["id"] == billing["id"])["keys"][0]
    assert retained["secret"] is None and retained["revoked_at"]
    assert retained["masked_secret"].startswith(old["secret"][:7])
    assert retained["masked_secret"].endswith(old["secret"][-4:])
    with sqlite3.connect(server.db) as db:
        secret, digest = db.execute("SELECT secret, secret_hash FROM sdk_keys WHERE id = ?", (old["id"],)).fetchone()
        assert secret is None and digest == hashlib.sha256(old["secret"].encode()).hexdigest()
    audits = admin.get("/api/environments/development/audits").json()["audits"]
    assert {entry["item_type"] for entry in audits} == {"app", "flag", "api_key"}
    assert all(entry["actor"] == "admin" for entry in audits)
    assert old["secret"] not in str(audits) and replacement["secret"] not in str(audits)
    revocations = admin.get("/api/environments/development/audits", params={"action": "revoked", "application_id": billing["id"]}).json()["audits"]
    assert len(revocations) == 1 and revocations[0]["item_id"] == old["id"]
    assert admin.get("/api/environments/production/audits", params={"action": "revoked"}).json()["audits"] == []
    older = admin.get("/api/environments/development/audits", params={"before": audits[-1]["id"]}).json()["audits"]
    assert older == []
    assert httpx.get(f"{server.url}/api/environments/development/audits", trust_env=False).status_code == 401


def test_confirmation_is_exact_and_rejects_stale_impact(admin):
    path = "/api/environments/development/flags/shared"
    assert admin.post("/api/environments/development/flags", json={"key": "shared", "mode": "enabled"}).status_code == 201
    impact = admin.get(f"{path}/impact").json()
    assert impact["applications"] == []
    for confirmation in ("", "shared", "production/shared", "development/Shared"):
        result = admin.put(path, json={"mode": "disabled"}, headers={"X-Partly-Confirm": confirmation, "X-Partly-Impact": impact["token"]})
        assert result.status_code == 400
    app = create_app(admin, "Billing", ["shared"])
    confirmed = {"X-Partly-Confirm": "development/shared", "X-Partly-Impact": impact["token"]}
    assert admin.put(path, json={"mode": "disabled"}, headers=confirmed).status_code == 409
    assert admin.delete(path, headers=confirmed).status_code == 409
    impact = admin.get(f"{path}/impact").json()
    assert impact["applications"] == [{"id": app["id"], "name": "Billing"}]
    confirmed["X-Partly-Impact"] = impact["token"]
    assert admin.put(path, json={"mode": "disabled"}, headers=confirmed).status_code == 200
    assert admin.put(path, json={"mode": "enabled"}, headers=confirmed).status_code == 409
    audit = admin.get("/api/environments/development/audits").json()["audits"][0]
    assert audit["details"]["before"] == {"mode": "enabled"}
    assert audit["details"]["after"] == {"mode": "disabled"}
    assert audit["details"]["applications"] == impact["applications"]
    assert admin.delete(path).status_code == 204
    assert admin.get("/api/environments/development/apps").json()["applications"][0]["flags"] == []


def test_invalid_assignment_and_audit_failure_roll_back(server, admin):
    app = create_app(admin, "Billing")
    path = f"/api/environments/development/apps/{app['id']}"
    before = admin.get("/api/environments/development/audits").json()
    assert admin.put(path, json={"name": "Changed", "flags": ["missing"]}).status_code == 400
    assert admin.get("/api/environments/development/apps").json()["applications"][0]["name"] == "Billing"
    assert admin.get("/api/environments/development/audits").json() == before
    assert admin.post("/api/environments/development/apps", json={"name": "Billing", "flags": []}).status_code == 409
    assert admin.put(f"/api/environments/production/apps/{app['id']}", json={"name": "Billing", "flags": []}).status_code == 404
    with sqlite3.connect(server.db) as db:
        db.execute("CREATE TRIGGER reject_audit BEFORE INSERT ON audits BEGIN SELECT RAISE(ABORT, 'audit write failure'); END")
    assert admin.put(path, json={"name": "Changed", "flags": []}).status_code == 500
    assert admin.post("/api/environments/development/flags", json={"key": "test", "mode": "enabled"}).status_code == 500
    assert admin.delete(f"/api/keys/{app['keys'][0]['id']}").status_code == 500
    restored = admin.get("/api/environments/development/apps").json()["applications"][0]
    assert restored["name"] == "Billing" and restored["keys"][0]["secret"] == app["keys"][0]["secret"]
    assert admin.get("/api/environments/development/flags").json() == {}
    assert admin.get("/api/environments/development/audits").json() == before


def test_audit_pagination_noop_and_revocation_survive_restart(server, admin):
    app = create_app(admin, "Billing")
    key = app["keys"][0]
    for index in range(101):
        result = admin.post("/api/environments/development/flags", json={"key": f"flag_{index}", "mode": "enabled"})
        assert result.status_code == 201
    page = admin.get("/api/environments/development/audits").json()["audits"]
    assert len(page) == 100
    older = admin.get("/api/environments/development/audits", params={"before": page[-1]["id"]}).json()["audits"]
    assert len(older) == 3
    assert len({entry["id"] for entry in page + older}) == 103
    assert admin.post("/api/environments/development/audits", json={"actor": "someone_else"}).status_code == 405
    assert admin.put(f"/api/environments/development/apps/{app['id']}", json={"name": "Billing", "flags": []}).status_code == 200
    assert admin.put("/api/environments/development/flags/flag_0", json={"mode": "enabled"}).status_code == 200
    assert admin.get("/api/environments/development/audits").json()["audits"] == page
    # Creating flags after the application does not grant access to them.
    with httpx.Client(base_url=server.url, trust_env=False) as sdk:
        policy = sdk.get("/api/policy", headers=headers(app))
        assert policy.json()["flags"] == {}
    assert admin.delete(f"/api/keys/{key['id']}").status_code == 204
    before = admin.get("/api/environments/development/audits").json()
    server.stop()
    server.start()
    with closing(server.admin(setup=False)) as recovered:
        retained = recovered.get("/api/environments/development/apps").json()["applications"][0]["keys"][0]
        assert retained["secret"] is None and retained["revoked_at"]
        assert retained["masked_secret"] == key["masked_secret"]
        assert recovered.get("/api/environments/development/audits").json() == before
    revoked = httpx.get(f"{server.url}/api/policy", headers=headers(app), trust_env=False)
    assert revoked.status_code == 401 and revoked.json()["code"] == "key_revoked"
