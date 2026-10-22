# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Elastic-2.0

import sqlite3
from contextlib import closing

import httpx
import pytest
from conftest import ADMIN_HEADERS, PASSWORD, confirm_request, make_key, save_flag


def create_user(admin, name, permissions=None, *, is_admin=False):
    body = {"username": name, "password": PASSWORD, "is_admin": is_admin, "enabled": True, "permissions": permissions or {}}
    response = admin.post("/api/users", json=body)
    assert response.status_code == 201, response.text
    assert "password" not in response.text
    return response.json()


def login_user(server, name, password=PASSWORD):
    client = httpx.Client(base_url=server.url, headers=ADMIN_HEADERS, trust_env=False)
    response = client.post("/api/login", json={"username": name, "password": password})
    assert response.status_code == 200, response.text
    client.event_hooks["request"].append(lambda request: confirm_request(client, request))
    return closing(client)


def update_user(admin, user, **changes):
    body = {key: value for key, value in user.items() if key != "id"}
    body.update(changes)
    return admin.put(f"/api/users/{user['id']}", json=body)


@pytest.mark.parametrize("role", ["viewer", "editor", "manager"])
def test_environment_permissions_are_enforced_on_every_api(server, admin, role):
    save_flag(admin, flag={"mode": "enabled"})
    _, secret = make_key(admin)
    prod_key, prod_secret = make_key(admin, "production")
    app = admin.get("/api/environments/development/apps").json()["applications"][0]
    key = app["keys"][0]
    user = create_user(admin, role, {"development": role})
    path = "/api/environments/development"
    with login_user(server, role) as client:
        access = client.get("/api/environments").json()
        assert access["environments"] == ["development"]
        assert access["user"]["username"] == role
        for resource in ("flags", "apps", "audits", "flags/new_checkout/impact"):
            assert client.get(f"{path}/{resource}").status_code == 200
            assert client.get(f"/api/environments/production/{resource}").status_code == 403
        response = client.get(f"{path}/apps")
        assert prod_secret not in response.text
        assert (secret in response.text) == (role == "manager")
        assert response.json()["applications"][0]["keys"][0]["masked_secret"] == key["masked_secret"]
        assert client.get("/api/users").status_code == 403
        assert update_user(client, user, is_admin=True, permissions={}).status_code == 403
        assert client.post("/api/users", json={"username": "intruder", "password": PASSWORD, "enabled": True, "is_admin": True, "permissions": {}}).status_code == 403
        assert client.put(f"/api/keys/{prod_key}", json={"name": "stolen"}).status_code == 403
        assert client.delete(f"/api/keys/{prod_key}").status_code == 403
        assert client.post("/api/environments/production/flags", json={"key": "denied", "mode": "enabled"}).status_code == 403
        expected = 403 if role == "viewer" else 201
        assert client.post(f"{path}/flags", json={"key": "new", "mode": "disabled"}).status_code == expected
        expected = 403 if role == "viewer" else 200
        assert client.put(f"{path}/flags/new_checkout", json={"mode": "disabled"}).status_code == expected
        expected = 403 if role == "viewer" else 204
        assert client.delete(f"{path}/flags/new_checkout").status_code == expected
        expected = 201 if role == "manager" else 403
        assert client.post(f"{path}/apps", json={"name": "Another app", "flags": []}).status_code == expected
        expected = 200 if role == "manager" else 403
        assert client.put(f"{path}/apps/{app['id']}", json={"name": "Renamed app", "flags": []}).status_code == expected
        assert client.post(f"{path}/apps/{app['id']}/keys", json={"name": "Rotation"}).status_code == expected
        expected = 204 if role == "manager" else 403
        assert client.put(f"/api/keys/{key['id']}", json={"name": "Renamed key"}).status_code == expected
        assert client.delete(f"/api/keys/{key['id']}").status_code == expected
        entries = client.get(f"{path}/audits").json()["audits"]
        own_entries = [entry for entry in entries if entry["actor"] == role]
        assert bool(own_entries) == (role != "viewer")
        assert secret not in str(entries) and prod_secret not in str(entries)
    assert admin.get("/api/environments/production/flags").json() == {}


def test_permission_changes_disabling_and_password_reset_invalidate_sessions(server, admin):
    user = create_user(admin, "teammate", {"development": "manager", "production": "viewer"})
    with login_user(server, "teammate") as first, login_user(server, "teammate") as second:
        assert update_user(admin, user, permissions={"production": "viewer"}).status_code == 200
        for client in (first, second):
            assert client.get("/api/environments").status_code == 401
    with login_user(server, "teammate") as client:
        assert client.get("/api/environments").json()["environments"] == ["production"]
        assert client.get("/api/environments/development/apps").status_code == 403
        response = update_user(admin, user, enabled=False)
        assert response.status_code == 200
        assert client.get("/api/environments").status_code == 401
        assert client.post("/api/login", json={"username": "teammate", "password": PASSWORD}).status_code == 401
    new_password = "replacement-test-password"
    assert update_user(admin, user, password=new_password).status_code == 200
    with httpx.Client(base_url=server.url, headers=ADMIN_HEADERS, trust_env=False) as client:
        assert client.post("/api/login", json={"username": "teammate", "password": PASSWORD}).status_code == 401
    server.stop()
    server.start()
    with login_user(server, "teammate", new_password) as client:
        assert client.get("/api/environments").json()["user"]["permissions"] == user["permissions"]
    with closing(server.admin(setup=False)) as client:
        events = client.get("/api/environments/development/audits").json()["audits"]
        assert all(event["actor"] == "admin" for event in events)
        assert events[0]["details"]["password_changed"] is True
        assert PASSWORD not in str(events) and new_password not in str(events) and "$argon2" not in str(events)


def test_initial_admin_and_account_validation(server, admin):
    root = admin.get("/api/users").json()["users"][0]
    assert update_user(admin, root, enabled=False).status_code == 409
    assert update_user(admin, root, is_admin=False).status_code == 409
    assert update_user(admin, root, username="renamed").status_code == 400
    assert admin.get("/api/environments").status_code == 200
    no_access = create_user(admin, "unassigned")
    with login_user(server, "unassigned") as client:
        assert client.get("/api/environments").json()["environments"] == []
        assert client.get("/api/environments/development/flags").status_code == 403
    body = {key: value for key, value in no_access.items() if key != "id"}
    assert admin.post("/api/users", json={**body, "password": PASSWORD}).status_code == 409
    for changes, status in [({"username": " spaces "}, 400), ({"username": "n", "password": "short"}, 400),
                            ({"permissions": {"production": "owner"}}, 422), ({"permissions": {"unknown": "viewer"}}, 400),
                            ({"is_admin": True, "permissions": {"production": "viewer"}}, 400)]:
        assert update_user(admin, no_access, **changes).status_code == status
    assert admin.post("/api/users", json={**body, "username": "missing_password"}).status_code == 400
    assert update_user(admin, {**no_access, "id": 999}).status_code == 404
    with httpx.Client(base_url=server.url, headers=ADMIN_HEADERS, trust_env=False) as client:
        assert client.get("/api/users").status_code == 401
        assert client.post("/api/users", json={**body, "password": PASSWORD}).status_code == 401
    # Administrators may delegate full access; a demotion affects their next request.
    other = create_user(admin, "other_admin", is_admin=True)
    with login_user(server, "other_admin") as client:
        assert client.get("/api/users").status_code == 200
        assert client.get("/api/environments").json()["environments"] == ["development", "production"]
        assert update_user(admin, other, is_admin=False).status_code == 200
        assert client.get("/api/users").status_code == 401


def test_user_change_and_audit_are_atomic(server, admin):
    user = create_user(admin, "viewer", {"development": "viewer"})
    with login_user(server, "viewer") as client:
        with sqlite3.connect(server.db) as db:
            db.execute("CREATE TRIGGER fail_user_audit BEFORE INSERT ON audits BEGIN SELECT RAISE(ABORT, 'test'); END")
        assert update_user(admin, user, enabled=False, permissions={}).status_code == 500
        assert client.get("/api/environments/development/flags").status_code == 200
        stored = next(row for row in admin.get("/api/users").json()["users"] if row["id"] == user["id"])
        assert stored == user


def test_users_dashboard_and_read_only_access(server, admin):
    from playwright.sync_api import expect, sync_playwright
    from test_dashboard import choose, launch_browser, login

    save_flag(admin, flag={"mode": "enabled"})
    _, secret = make_key(admin)
    with sync_playwright() as playwright:
        browser = launch_browser(playwright)
        page = browser.new_page(viewport={"width": 1280, "height": 900})
        errors = []
        page.on("pageerror", lambda error: errors.append(str(error)))
        login(page, server, "users")
        requests = []
        page.on("request", lambda request: requests.append((request.method, request.url)))
        page.get_by_role("button", name="Create user", exact=True).click()
        dialog = page.get_by_role("dialog", name="Create user", exact=True)
        dialog.get_by_label("Username", exact=True).fill("reader")
        dialog.get_by_label("Password", exact=True).fill(PASSWORD)
        choose(page, dialog.get_by_label("development access", exact=True), "viewer")
        dialog.get_by_role("button", name="Create user", exact=True).click()
        expect(dialog).to_be_hidden()
        expect(page.get_by_role("button", name="Edit reader", exact=True)).to_be_visible()
        expect(page.get_by_role("button", name="Edit reader", exact=True)).to_be_focused()
        assert ("GET", f"{server.url}/api/users") not in requests
        page.get_by_role("button", name="Edit reader", exact=True).click()
        dialog = page.get_by_role("dialog", name="Edit reader", exact=True)
        expect(dialog.get_by_label("Username", exact=True)).to_have_attribute("readonly", "")
        expect(dialog.get_by_role("button", name="Save changes", exact=True)).to_be_disabled()
        choose(page, dialog.get_by_label("production access", exact=True), "editor")
        dialog.get_by_role("button", name="Cancel", exact=True).click()
        expect(page.get_by_role("dialog", name="Discard your changes?")).to_be_visible()
        page.get_by_role("button", name="Discard changes", exact=True).click()
        expect(dialog).to_be_hidden()
        page.get_by_role("button", name="Edit admin", exact=True).click()
        expect(page.get_by_label("Instance administrator", exact=True)).to_be_disabled()
        expect(page.get_by_label("Account enabled", exact=True)).to_be_disabled()
        page.get_by_role("dialog", name="Edit admin").get_by_role("button", name="Cancel", exact=True).click()
        for width in (1280, 390):
            page.set_viewport_size({"width": width, "height": 900})
            assert page.evaluate("document.documentElement.scrollWidth <= innerWidth")
        page.get_by_role("button", name="Sign out", exact=True).click()
        page.goto(server.url)
        page.locator("#auth").get_by_label("Username", exact=True).fill("reader")
        page.locator("#auth").get_by_label("Password", exact=True).fill(PASSWORD)
        page.get_by_role("button", name="Sign in", exact=True).click()
        expect(page).to_have_url(f"{server.url}/environments/development/flags")
        expect(page.get_by_role("tab", name="Users", exact=True)).to_have_count(0)
        expect(page.get_by_role("button", name="Create flag", exact=True)).to_be_hidden()
        expect(page.get_by_label("Rollout mode for new_checkout", exact=True)).to_be_disabled()
        page.get_by_label("Environment", exact=True).click()
        expect(page.locator("#dropdown-options").get_by_role("option")).to_have_text(["Development"])
        page.keyboard.press("Escape")
        page.get_by_role("tab", name="Apps & Keys", exact=True).click()
        expect(page.locator(".key-row")).to_be_visible()
        expect(page.get_by_role("button", name="Reveal", exact=True)).to_have_count(0)
        expect(page.get_by_role("button", name="Create app", exact=True)).to_be_hidden()
        assert secret not in page.content()
        page.get_by_role("tab", name="Feature flags", exact=True).focus()
        page.keyboard.press("End")
        expect(page.get_by_role("tab", name="Docs", exact=True)).to_be_focused()
        page.goto(f"{server.url}/environments/production/apps")
        expect(page.get_by_role("alert")).to_contain_text("You do not have access to this environment")
        expect(page.locator("#workspace")).to_be_hidden()
        page.locator(".brand").click()
        expect(page).to_have_url(f"{server.url}/environments/development/flags")
        assert not errors
        browser.close()


def test_user_without_environment_gets_clear_access_message(server, admin):
    from playwright.sync_api import expect, sync_playwright
    from test_dashboard import launch_browser

    create_user(admin, "unassigned")
    with sync_playwright() as playwright:
        browser = launch_browser(playwright)
        page = browser.new_page()
        page.goto(server.url)
        page.locator("#auth").get_by_label("Username", exact=True).fill("unassigned")
        page.locator("#auth").get_by_label("Password", exact=True).fill(PASSWORD)
        page.get_by_role("button", name="Sign in", exact=True).click()
        expect(page.get_by_role("alert")).to_contain_text("Ask an administrator to assign access")
        expect(page.locator("#workspace")).to_be_hidden()
        expect(page.get_by_role("button", name="Sign out", exact=True)).to_be_visible()
        browser.close()


def test_password_reset_ends_sessions_without_revoking_application_keys(server, admin):
    user = create_user(admin, "manager", {"development": "manager"})
    with login_user(server, "manager") as client:
        response = client.post("/api/environments/development/apps", json={"name": "Service", "flags": []})
        assert response.status_code == 201
        app = response.json()
        assert update_user(admin, user, password="new-test-password").status_code == 200
        assert client.get("/api/environments/development/flags").status_code == 401
    with login_user(server, "manager", "new-test-password") as client:
        assert client.get("/api/environments/development/flags").status_code == 200
        assert update_user(admin, user, enabled=False).status_code == 200
        assert client.get("/api/environments/development/flags").status_code == 401
    with httpx.Client(base_url=server.url, trust_env=False) as sdk:
        headers = {"Authorization": f"Bearer {app['keys'][0]['secret']}", "X-Partly-Environment": "development", "X-Partly-Application": app["id"]}
        assert sdk.get("/api/policy", headers=headers).status_code == 200
        assert sdk.get("/api/users", headers=headers).status_code == 401
