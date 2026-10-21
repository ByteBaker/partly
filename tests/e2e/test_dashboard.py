# Copyright 2026 ByteBaker
# SPDX-License-Identifier: Elastic-2.0

import asyncio
import os
import re

from conftest import PASSWORD, ROOT, scope_for
from playwright.sync_api import expect, sync_playwright

from partly import Client, Decision


def launch_browser(playwright):
    engine = os.environ.get("PARTLY_BROWSER", "chromium")
    assert engine in {"chromium", "firefox", "webkit"}, f"Unknown browser: {engine}"
    return getattr(playwright, engine).launch()


def login(page, server, screen="flags", environment="development"):
    page.goto(f"{server.url}/environments/{environment}/{screen}")
    page.get_by_label("Username", exact=True).fill("admin")
    page.get_by_label("Password", exact=True).fill(PASSWORD)
    page.get_by_role("button", name="Sign in", exact=True).click()
    expect(page.locator("#workspace")).to_be_visible()


def choose(page, trigger, value):
    trigger.click()
    popup = page.locator("#dropdown-options")
    expect(popup).to_be_visible()
    popup.locator(f'[role="option"][data-value="{value}"]').click()


def expect_active_option(page, trigger, value):
    expect(trigger).to_have_attribute("aria-activedescendant", re.compile(r".+"))
    active_id = trigger.get_attribute("aria-activedescendant")
    expect(page.locator(f"#{active_id}")).to_have_attribute("data-value", value)


def create_flag(page, key, mode="disabled", percentage=None):
    page.get_by_role("button", name="Create flag", exact=True).click()
    dialog = page.get_by_role("dialog", name="Create a flag")
    dialog.get_by_label("Flag key", exact=True).fill(key)
    choose(page, dialog.get_by_label("Rollout mode for new flag", exact=True), mode)
    if percentage is not None:
        dialog.get_by_label("Percentage for new flag", exact=True).fill(str(percentage))
    dialog.get_by_role("button", name="Create flag", exact=True).click()
    expect(dialog).to_be_hidden()
    expect(page.locator(f'.flag-row[data-key="{key}"]')).to_be_visible()


def ensure_app(page):
    if page.locator(".app-row").count():
        return False
    page.get_by_role("button", name="Create app", exact=True).click()
    dialog = page.get_by_role("dialog", name="Create app", exact=True)
    dialog.get_by_label("Application name", exact=True).fill("Test application")
    for checkbox in dialog.get_by_role("checkbox").all():
        checkbox.check()
    dialog.get_by_role("button", name="Create app", exact=True).click()
    expect(dialog).to_be_hidden()
    expect(page.locator(".app-row")).to_be_visible()
    return True


def open_key(page):
    ensure_app(page)
    page.get_by_role("button", name="Create key", exact=True).click()


def create_key(page, name):
    if ensure_app(page):
        page.get_by_label("Name for Default", exact=True).fill(name)
        page.get_by_role("button", name="Save name for Default", exact=True).click()
    else:
        open_key(page)
        dialog = page.get_by_role("dialog", name="Generate API Key", exact=True)
        dialog.get_by_label("Key name", exact=True).fill(name)
        dialog.get_by_role("button", name="Generate key", exact=True).click()
        expect(dialog).to_be_hidden()
    expect(page.get_by_label(f"Name for {name}", exact=True)).to_be_visible()


def confirm_flag(page, key):
    environment = page.url.split("/environments/")[1].split("/")[0]
    modal = page.locator("#confirm-flag-dialog")
    expect(modal).to_be_visible()
    modal.locator("#confirm-flag-input").fill(f"{environment}/{key}")
    modal.locator("#confirm-flag-submit").click()
    expect(modal).to_be_hidden()


def test_dashboard_setup_flags_keys_and_restart(server):
    with sync_playwright() as playwright:
        browser = launch_browser(playwright)
        page = browser.new_page(viewport={"width": 1280, "height": 1000}, permissions=["clipboard-read", "clipboard-write"])
        errors = []
        page.on("pageerror", lambda error: errors.append(str(error)))
        page.goto(server.url)
        expect(page.get_by_role("button", name="Create admin account")).to_be_visible()
        assert not server.db.exists()
        page.get_by_label("Username", exact=True).fill("admin")
        page.get_by_label("Password", exact=True).fill(PASSWORD)
        page.get_by_role("button", name="Create admin account").click()
        expect(page).to_have_url(f"{server.url}/environments/development/flags")
        assert server.db.exists()
        create_flag(page, "new_checkout", "percentage", 25)
        page.get_by_label("Percentage for new_checkout").fill("50")
        page.get_by_role("button", name="Save new_checkout", exact=True).click()
        confirm_flag(page, "new_checkout")
        expect(page.get_by_role("status")).to_contain_text("Saved")
        expect(page.get_by_role("tab")).to_have_text(["Feature flags", "Apps & Keys", "Audits", "Docs"])
        artifacts = ROOT / "test-results"
        artifacts.mkdir(exist_ok=True)
        page.screenshot(path=str(artifacts / "dashboard-desktop.png"), full_page=True)
        page.get_by_role("tab", name="Apps & Keys").click()
        expect(page).to_have_url(f"{server.url}/environments/development/apps")
        heading = page.locator(".workspace-heading").bounding_box()
        create_key(page, "Checkout service")
        expect(page.locator(".key-row").get_by_role("status")).to_have_text("Saved")
        assert page.locator(".workspace-heading").bounding_box() == heading
        field = page.locator(".key-controls input")
        expect(field).to_have_attribute("type", "text")
        expect(field).to_have_value(re.compile(r"pk_[A-Za-z0-9]{4}•+[A-Za-z0-9]{4}"))
        requests = []
        page.on("request", lambda request: requests.append(request.url))
        copy = page.get_by_role("button", name=re.compile("^Copy API Key "))
        copy.click()
        expect(page.get_by_role("status")).to_have_text("Copied")
        expect(page.locator(".key-row").get_by_role("status")).to_have_text("Copied")
        assert page.locator(".workspace-heading").bounding_box() == heading
        expect(copy).to_have_text("Copy")
        secret = page.evaluate("navigator.clipboard.readText()")
        assert re.fullmatch(r"pk_[A-Za-z0-9]{24}", secret)
        expect(field).to_have_value(secret[:7] + "•" * 12 + secret[-4:])
        expect(field).to_have_attribute("type", "text")
        page.get_by_role("button", name="Reveal", exact=True).click()
        expect(field).to_have_attribute("type", "text")
        page.get_by_role("button", name="Hide", exact=True).click()
        expect(field).to_have_attribute("type", "text")
        assert requests == []
        assert page.locator(".key-row code").count() == 0
        page.get_by_label("Name for Checkout service", exact=True).fill("Checkout worker")
        page.get_by_role("button", name="Save name for Checkout service").click()
        expect(page.get_by_label("Name for Checkout worker", exact=True)).to_have_value("Checkout worker")
        expect(page.locator(".key-row").get_by_role("status")).to_have_text("Saved")
        server.stop()
        server.start()
        page.reload()
        page.get_by_label("Username", exact=True).fill("admin")
        page.get_by_label("Password", exact=True).fill(PASSWORD)
        page.get_by_role("button", name="Sign in", exact=True).click()
        expect(page.get_by_label("Name for Checkout worker", exact=True)).to_have_value("Checkout worker")
        expect(field).to_have_attribute("type", "text")
        expect(field).to_have_value(secret[:7] + "•" * 12 + secret[-4:])
        copy.focus()
        page.keyboard.press("Enter")
        expect(page.get_by_role("status")).to_have_text("Copied")
        assert page.evaluate("navigator.clipboard.readText()") == secret
        page.screenshot(path=str(artifacts / "keys-desktop.png"), full_page=True)
        page.set_viewport_size({"width": 390, "height": 844})
        page.screenshot(path=str(artifacts / "keys-mobile.png"), full_page=True)
        assert page.evaluate("document.documentElement.scrollWidth <= innerWidth")
        page.evaluate("""Object.defineProperty(navigator.clipboard, 'writeText', {
            configurable: true,
            value: async () => { throw new DOMException('Clipboard blocked', 'NotAllowedError'); }
        })""")
        copy.click()
        expect(page.get_by_role("alert")).to_contain_text("Could not copy")
        expect(field).to_have_attribute("type", "text")
        page.evaluate("delete navigator.clipboard.writeText")
        choose(page, page.get_by_label("Environment", exact=True), "production")
        expect(page).to_have_url(f"{server.url}/environments/production/apps")
        expect(page.get_by_text("No applications in this environment.")).to_be_visible()
        page.get_by_role("tab", name="Feature flags").click()
        expect(page.get_by_text("No flags in this environment")).to_be_visible()
        choose(page, page.get_by_label("Environment", exact=True), "development")
        expect(page.get_by_label("Percentage for new_checkout")).to_have_value("50")
        page.screenshot(path=str(artifacts / "dashboard-mobile.png"), full_page=True)
        page.on("dialog", lambda dialog: dialog.accept())
        page.get_by_role("button", name="Delete new_checkout").click()
        confirm_flag(page, "new_checkout")
        expect(page.get_by_text("No flags in this environment")).to_be_visible()
        page.get_by_role("tab", name="Apps & Keys").click()
        page.get_by_role("button", name="Revoke", exact=True).click()
        expect(page.get_by_text("Revoked keys (1)")).to_be_visible()
        page.get_by_role("button", name="Sign out").click()
        expect(page.get_by_role("button", name="Sign in", exact=True)).to_be_visible()
        expect(page.get_by_role("tablist")).to_be_hidden()
        assert page.locator(".key-controls input").count() == 0
        assert not errors
        browser.close()


def test_docs_navigation_and_complete_example(server, admin):
    with sync_playwright() as playwright:
        browser = launch_browser(playwright)
        page = browser.new_page(viewport={"width": 1280, "height": 1000}, permissions=["clipboard-read", "clipboard-write"])
        login(page, server)
        flags = page.get_by_role("tab", name="Feature flags")
        keys = page.get_by_role("tab", name="Apps & Keys")
        docs = page.get_by_role("tab", name="Docs")
        flags.focus()
        page.keyboard.press("ArrowRight")
        expect(keys).to_be_focused()
        expect(keys).to_have_attribute("aria-selected", "true")
        page.keyboard.press("End")
        expect(docs).to_be_focused()
        expect(docs).to_have_attribute("aria-selected", "true")
        expect(page.get_by_label("Language", exact=True)).to_have_attribute("value", "python")
        page.get_by_label("Language", exact=True).click()
        expect(page.locator("#dropdown-options").get_by_role("option")).to_have_count(1)
        expect(page.locator("#dropdown-options").get_by_role("option", name="Python", exact=True)).to_be_visible()
        page.keyboard.press("Escape")
        expect(page.get_by_role("button", name="Copy code")).to_be_enabled()
        expect(page.locator("#docs-code .token-keyword").first).to_be_visible()
        expect(page.locator("#docs-code .token-string").first).to_be_visible()
        expect(page.get_by_role("button", name="Copy code")).to_have_attribute("title", "Copy code")
        code = (ROOT / "examples/python/quickstart.py").read_text()
        assert page.locator("#docs-code").text_content() == code
        page.get_by_role("button", name="Copy code").click()
        expect(page.get_by_role("status")).to_have_text("Copied")
        expect(page.locator(".code-heading").get_by_role("status")).to_have_text("Copied")
        assert page.evaluate("navigator.clipboard.readText()") == code
        artifacts = ROOT / "test-results"
        artifacts.mkdir(exist_ok=True)
        page.screenshot(path=str(artifacts / "docs-desktop.png"), full_page=True)
        page.set_viewport_size({"width": 390, "height": 844})
        page.screenshot(path=str(artifacts / "docs-mobile.png"), full_page=True)
        assert page.evaluate("document.documentElement.scrollWidth <= innerWidth")
        docs.focus()
        page.keyboard.press("Home")
        expect(flags).to_be_focused()
        page.keyboard.press("ArrowLeft")
        expect(docs).to_have_attribute("aria-selected", "true")
        expect(page.get_by_role("tabpanel", name="Feature flags")).to_be_hidden()
        browser.close()


def test_flag_modal_types_rules_and_in_place_saves(server, admin):
    with sync_playwright() as playwright:
        browser = launch_browser(playwright)
        page = browser.new_page(viewport={"width": 1280, "height": 1000})
        login(page, server)
        create_flag(page, "other_flag", "percentage", 50)
        page.get_by_label("Percentage for other_flag").fill("75")
        requests = []
        page.on("request", lambda request: requests.append((request.method, request.url)))
        page.get_by_role("button", name="Create flag", exact=True).click()
        modal = page.get_by_role("dialog", name="Create a flag")
        expect(modal.get_by_text("Create flag in development.", exact=True)).to_be_visible()
        assert modal.get_by_label("Environment", exact=True).count() == 0
        expect(modal.get_by_label("Flag key", exact=True)).to_be_focused()
        modal.get_by_label("Flag key", exact=True).fill("eligible")
        choose(page, modal.get_by_label("Rollout mode for new flag"), "custom")
        first = modal.locator(".condition").first
        first.get_by_label("Context key", exact=True).fill("age")
        for op in ("le", "lt", "ge", "gt", "eq"):
            choose(page, first.get_by_label("Operator", exact=True), op)
            expect(first.get_by_label("Comparison value", exact=True)).to_have_attribute("type", "number")
        expect(first.get_by_text(re.compile("ContextTypeError"))).to_be_visible()
        first.get_by_label("Comparison value", exact=True).fill("9007199254740993")
        modal.get_by_role("button", name="Create flag", exact=True).click()
        expect(modal).to_be_visible()
        assert first.get_by_label("Comparison value", exact=True).evaluate("e => !e.checkValidity()")
        first.get_by_label("Comparison value", exact=True).fill("-18.5")
        for op in ("prefix", "suffix", "contains", "matches", "regex"):
            choose(page, first.get_by_label("Operator", exact=True), op)
            expect(first.get_by_label("Comparison value", exact=True)).to_have_attribute("type", "text")
        expect(first.get_by_label("Comparison value", exact=True)).to_have_value("")
        choose(page, first.get_by_label("Operator", exact=True), "ge")
        first.get_by_label("Comparison value", exact=True).fill("18")
        modal.get_by_role("button", name="+ AND condition", exact=True).click()
        second = modal.locator(".condition").nth(1)
        second.get_by_label("Context key", exact=True).fill("verified")
        choose(page, second.get_by_label("Operator", exact=True), "is")
        expect(second.get_by_role("radio", name="True", exact=True)).not_to_be_checked()
        expect(second.get_by_role("radio", name="False", exact=True)).not_to_be_checked()
        second.get_by_role("radio", name="True", exact=True).check()
        modal.get_by_role("button", name="+ OR group", exact=True).click()
        third = modal.locator(".condition").nth(2)
        third.get_by_label("Context key", exact=True).fill("plan")
        third.get_by_label("Comparison value", exact=True).fill("pro")
        modal.get_by_role("button", name="Create flag", exact=True).click()
        expect(modal).to_be_hidden()
        expect(page.get_by_label("Percentage for other_flag")).to_have_value("75")
        row = page.locator('.flag-row[data-key="eligible"]')
        choose(page, row.get_by_label("Match action for eligible"), "disable")
        page.get_by_role("button", name="Save eligible", exact=True).click()
        confirm_flag(page, "eligible")
        expect(page.get_by_role("status")).to_contain_text("Saved")
        expect(page.get_by_label("Percentage for other_flag")).to_have_value("75")
        expect(page.get_by_role("button", name="Save other_flag", exact=True)).to_be_enabled()
        assert not [request for request in requests if request[0] == "GET" and not request[1].endswith("/impact")]
        saved = admin.get("/api/environments/development/flags").json()["eligible"]
        assert saved == {"mode": "custom", "action": "disable", "groups": [
            [{"k": "age", "op": "ge", "v": 18}, {"k": "verified", "op": "is", "v": True}],
            [{"k": "plan", "op": "matches", "v": "pro"}],
        ]}
        # Duplicate creation stays in the modal with its draft intact.
        page.get_by_role("button", name="Create flag", exact=True).click()
        modal.get_by_label("Flag key", exact=True).fill("eligible")
        modal.get_by_role("button", name="Create flag", exact=True).click()
        expect(modal.get_by_role("alert")).to_be_visible()
        expect(modal.get_by_label("Flag key", exact=True)).to_have_value("eligible")
        page.on("dialog", lambda dialog: dialog.accept())
        browser.close()


async def test_dashboard_rules_and_key_drive_sdk_refresh(server, admin):
    def create_in_dashboard():
        with sync_playwright() as playwright:
            browser = launch_browser(playwright)
            try:
                page = browser.new_page()
                login(page, server)
                page.get_by_role("button", name="Create flag", exact=True).click()
                modal = page.get_by_role("dialog", name="Create a flag")
                modal.get_by_label("Flag key", exact=True).fill("eligible")
                choose(page, modal.get_by_label("Rollout mode for new flag"), "custom")
                age = modal.locator(".condition").first
                age.get_by_label("Context key", exact=True).fill("age")
                choose(page, age.get_by_label("Operator", exact=True), "ge")
                age.get_by_label("Comparison value", exact=True).fill("18")
                modal.get_by_role("button", name="+ AND condition", exact=True).click()
                verified = modal.locator(".condition").nth(1)
                verified.get_by_label("Context key", exact=True).fill("verified")
                choose(page, verified.get_by_label("Operator", exact=True), "is")
                verified.get_by_role("radio", name="True", exact=True).check()
                modal.get_by_role("button", name="Create flag", exact=True).click()
                expect(modal).to_be_hidden()
                page.get_by_role("tab", name="Apps & Keys").click()
                create_key(page, "SDK application")
                page.get_by_role("button", name="Reveal", exact=True).click()
                return page.get_by_label("API Key for SDK application", exact=True).input_value(), page.locator(".app-row").get_attribute("data-app-id")
            finally:
                browser.close()

    def edit_in_dashboard():
        with sync_playwright() as playwright:
            browser = launch_browser(playwright)
            try:
                page = browser.new_page()
                login(page, server)
                age = page.locator('.flag-row[data-key="eligible"] .condition').first
                age.get_by_label("Comparison value", exact=True).fill("30")
                page.get_by_role("button", name="Save eligible", exact=True).click()
                confirm_flag(page, "eligible")
                expect(page.get_by_role("status")).to_have_text("Saved")
            finally:
                browser.close()

    # Each browser session stays in its worker thread; the SDK uses this event loop.
    secret, application_id = await asyncio.to_thread(create_in_dashboard)
    client = Client.builder().url(server.url).environment(scope_for(secret)[0]).application_id(application_id).secret_key(secret).poll_interval(3600).build()
    eligible = {"age": 25, "verified": True}
    try:
        await client.init()
        assert client.evaluate("eligible", context=eligible) is Decision.ENABLED
        assert client.evaluate("eligible", context={"age": 17, "verified": True}) is Decision.DISABLED
        assert client.evaluate("eligible", context={"age": 25, "verified": False}) is Decision.DISABLED
        assert await client.refresh() is False
        await asyncio.to_thread(edit_in_dashboard)
        assert client.evaluate("eligible", context=eligible) is Decision.ENABLED
        assert await client.refresh() is True
        assert client.evaluate("eligible", context=eligible) is Decision.DISABLED
        assert client.evaluate("eligible", context={"age": 35, "verified": True}) is Decision.ENABLED
        assert await client.refresh() is False
    finally:
        await client.aclose()


def test_unsaved_navigation_reload_and_modal_protection(server, admin):
    with sync_playwright() as playwright:
        browser = launch_browser(playwright)
        page = browser.new_page()
        login(page, server)
        create_flag(page, "checkout", "percentage", 50)
        field = page.get_by_label("Percentage for checkout")
        field.fill("")  # Invalid/incomplete drafts still need protection.
        choose(page, page.get_by_label("Environment", exact=True), "production")
        discard = page.get_by_role("dialog", name="Discard your changes?")
        expect(discard).to_be_visible()
        page.get_by_role("button", name="Keep editing").click()
        expect(field).to_have_value("")
        expect(page.get_by_label("Environment", exact=True)).to_have_attribute("value", "development")
        field.fill("50")
        expect(page.get_by_role("button", name="Save checkout", exact=True)).to_be_disabled()
        # Returning to the baseline removes the warning entirely.
        page.evaluate("window.reloadMarker = true")
        choose(page, page.get_by_label("Environment", exact=True), "production")
        expect(page).to_have_url(f"{server.url}/environments/production/flags")
        assert page.evaluate("window.reloadMarker === undefined")
        choose(page, page.get_by_label("Environment", exact=True), "development")
        field.fill("75")
        page.get_by_role("tab", name="Docs").click()
        expect(discard).to_be_visible()
        page.get_by_role("button", name="Discard changes").click()
        expect(page).to_have_url(f"{server.url}/environments/development/docs")
        page.get_by_role("tab", name="Feature flags").click()
        expect(field).to_have_value("50")
        field.fill("75")
        seen_dialogs = []
        def cancel_reload(dialog):
            seen_dialogs.append(dialog.type)
            dialog.dismiss()
        page.on("dialog", cancel_reload)
        with page.expect_event("dialog"):
            page.evaluate("setTimeout(() => location.reload(), 0)")
        assert seen_dialogs == ["beforeunload"]
        expect(field).to_have_value("75")
        page.remove_listener("dialog", cancel_reload)
        field.fill("50")
        page.get_by_role("button", name="Create flag", exact=True).click()
        modal = page.get_by_role("dialog", name="Create a flag")
        modal.get_by_label("Flag key", exact=True).fill("draft")
        page.keyboard.press("Escape")
        expect(discard).to_be_visible()
        page.get_by_role("button", name="Keep editing").click()
        expect(modal).to_be_visible()
        expect(modal.get_by_label("Flag key", exact=True)).to_have_value("draft")
        # Revert every modal edit, then dismiss without confirmation.
        modal.get_by_label("Flag key", exact=True).fill("")
        modal.get_by_role("button", name="Cancel", exact=True).click()
        expect(modal).to_be_hidden()
        expect(discard).to_be_hidden()
        page.get_by_role("tab", name="Apps & Keys").click()
        open_key(page)
        page.get_by_label("Key name", exact=True).fill("unsaved application")
        page.keyboard.press("Escape")
        expect(discard).to_be_visible()
        page.get_by_role("button", name="Keep editing").click()
        expect(page.get_by_label("Key name", exact=True)).to_have_value("unsaved application")
        page.on("dialog", lambda dialog: dialog.accept())
        browser.close()


def test_failed_save_preserves_draft_and_key_rename_warns(server, admin):
    with sync_playwright() as playwright:
        browser = launch_browser(playwright)
        page = browser.new_page()
        login(page, server)
        create_flag(page, "checkout", "percentage", 50)
        page.get_by_label("Percentage for checkout").fill("75")
        page.route("**/api/environments/development/flags/checkout", lambda route: route.fulfill(status=500, json={"error": "Temporary failure"}))
        page.get_by_role("button", name="Save checkout", exact=True).click()
        confirm_flag(page, "checkout")
        expect(page.get_by_role("alert")).to_have_text("Temporary failure")
        expect(page.get_by_label("Percentage for checkout")).to_have_value("75")
        expect(page.get_by_role("button", name="Save checkout", exact=True)).to_be_enabled()
        page.get_by_role("tab", name="Apps & Keys").click()
        page.get_by_role("button", name="Discard changes").click()
        create_key(page, "Worker")
        name = page.get_by_label("Name for Worker", exact=True)
        name.fill("Renamed worker")
        choose(page, page.get_by_label("Environment", exact=True), "production")
        expect(page.get_by_role("dialog", name="Discard your changes?")).to_be_visible()
        page.get_by_role("button", name="Keep editing").click()
        expect(name).to_have_value("Renamed worker")
        name.fill("Worker")
        expect(page.get_by_role("button", name="Save name for Worker", exact=True)).to_be_disabled()
        page.get_by_role("tab", name="Docs").click()
        expect(page).to_have_url(f"{server.url}/environments/development/docs")
        browser.close()


def test_pending_save_locks_only_submitted_form(server, admin):
    with sync_playwright() as playwright:
        browser = launch_browser(playwright)
        page = browser.new_page()
        login(page, server)
        create_flag(page, "checkout", "percentage", 50)
        create_flag(page, "search", "percentage", 25)
        pending = []
        url = "**/api/environments/development/flags/checkout"
        page.route(url, lambda route: pending.append(route))
        field = page.get_by_label("Percentage for checkout")
        field.fill("75")
        page.get_by_role("button", name="Save checkout", exact=True).click()
        confirm_flag(page, "checkout")
        expect(field).to_be_disabled()
        expect(page.get_by_label("Rollout mode for checkout", exact=True)).to_be_disabled()
        other = page.get_by_label("Percentage for search")
        expect(other).to_be_enabled()
        other.fill("40")
        assert len(pending) == 1
        pending.pop().continue_()
        expect(field).to_be_enabled()
        expect(field).to_have_value("75")
        expect(page.get_by_role("button", name="Save checkout", exact=True)).to_be_disabled()
        expect(other).to_have_value("40")
        expect(other).to_be_focused()
        expect(page.get_by_role("button", name="Save search", exact=True)).to_be_enabled()
        page.on("dialog", lambda dialog: dialog.accept())
        browser.close()


def test_key_requests_are_serialized_and_session_failure_keeps_draft(server, admin):
    with sync_playwright() as playwright:
        browser = launch_browser(playwright)
        page = browser.new_page()
        errors = []
        page.on("pageerror", lambda error: errors.append(str(error)))
        login(page, server, screen="apps")
        create_key(page, "Worker")
        name = page.get_by_label("Name for Worker", exact=True)
        name.fill("Worker v2")
        pending = []
        page.route("**/api/keys/*", lambda route: pending.append(route))
        page.get_by_role("button", name="Save name for Worker", exact=True).click()
        expect(name).to_be_disabled()
        expect(page.get_by_role("button", name="Revoke", exact=True)).to_be_disabled()
        expect(page.get_by_role("button", name="Reveal", exact=True)).to_be_disabled()
        assert len(pending) == 1
        pending.pop().fulfill(status=401, json={"error": "Unauthorized"})
        expect(page.locator(".key-row").get_by_role("alert")).to_contain_text("Your session has ended")
        expect(name).to_have_value("Worker v2")
        expect(name).to_be_enabled()
        expect(page.get_by_role("button", name="Revoke", exact=True)).to_be_enabled()
        page.get_by_role("tab", name="Feature flags").click()
        expect(page.get_by_role("dialog", name="Discard your changes?")).to_be_visible()
        page.get_by_role("button", name="Keep editing").click()
        page.unroute("**/api/keys/*")
        page.get_by_role("button", name="Save name for Worker", exact=True).click()
        expect(page.get_by_label("Name for Worker v2", exact=True)).to_have_value("Worker v2")
        expect(page.get_by_label("Name for Worker v2", exact=True)).to_be_focused()
        assert not errors
        browser.close()


def test_rule_keyboard_focus_and_independent_row_errors(server, admin):
    with sync_playwright() as playwright:
        browser = launch_browser(playwright)
        page = browser.new_page()
        login(page, server)
        create_flag(page, "checkout", "percentage", 50)
        create_flag(page, "search", "percentage", 25)
        page.get_by_label("Percentage for checkout").fill("75")
        page.route("**/api/environments/development/flags/checkout", lambda route: route.fulfill(status=500, json={"error": "Checkout could not be saved"}))
        page.get_by_role("button", name="Save checkout", exact=True).click()
        confirm_flag(page, "checkout")
        checkout = page.locator('.flag-row[data-key="checkout"]')
        expect(checkout.get_by_role("alert")).to_have_text("Checkout could not be saved")
        page.get_by_label("Percentage for search").fill("40")
        page.get_by_role("button", name="Save search", exact=True).click()
        confirm_flag(page, "search")
        expect(page.get_by_role("status")).to_contain_text("Saved")
        expect(page.get_by_label("Rollout mode for search", exact=True)).to_be_focused()
        expect(checkout.get_by_role("alert")).to_have_text("Checkout could not be saved")
        expect(page.get_by_label("Percentage for checkout")).to_have_value("75")
        page.get_by_role("button", name="Create flag", exact=True).click()
        modal = page.get_by_role("dialog", name="Create a flag")
        choose(page, modal.get_by_label("Rollout mode for new flag"), "custom")
        first_group = modal.get_by_role("group", name="Group 1 · Match all", exact=True)
        first_group.get_by_role("button", name="+ AND condition").focus()
        page.keyboard.press("Enter")
        expect(first_group.get_by_label("Context key", exact=True).nth(1)).to_be_focused()
        first_group.get_by_role("button", name="Remove condition", exact=True).nth(1).click()
        expect(first_group.get_by_label("Context key", exact=True)).to_be_focused()
        modal.get_by_role("button", name="+ OR group").focus()
        page.keyboard.press("Enter")
        second_group = modal.get_by_role("group", name="Group 2 · Match all", exact=True)
        expect(second_group.get_by_label("Context key", exact=True)).to_be_focused()
        second_group.get_by_role("button", name="Remove group", exact=True).click()
        expect(first_group.get_by_label("Context key", exact=True)).to_be_focused()
        page.keyboard.press("Escape")
        expect(page.get_by_role("button", name="Keep editing")).to_be_focused()
        page.keyboard.press("Escape")
        expect(first_group.get_by_label("Context key", exact=True)).to_be_focused()
        page.on("dialog", lambda dialog: dialog.accept())
        browser.close()


def test_browser_back_preserves_dirty_edits_and_forward_keeps_environment(server, admin):
    with sync_playwright() as playwright:
        browser = launch_browser(playwright)
        page = browser.new_page()
        login(page, server, environment="production")
        create_flag(page, "checkout", "percentage", 50)
        page.get_by_role("tab", name="Apps & Keys").click()
        page.get_by_role("tab", name="Feature flags").click()
        field = page.get_by_label("Percentage for checkout")
        field.fill("75")
        def cancel(dialog):
            assert dialog.type == "beforeunload"
            dialog.dismiss()
        page.on("dialog", cancel)
        with page.expect_event("dialog"):
            page.evaluate("setTimeout(() => history.back(), 0)")
        expect(page).to_have_url(f"{server.url}/environments/production/flags")
        expect(field).to_have_value("75")
        page.remove_listener("dialog", cancel)
        field.fill("50")
        page.go_back()
        expect(page).to_have_url(f"{server.url}/environments/production/apps")
        page.go_forward()
        expect(page).to_have_url(f"{server.url}/environments/production/flags")
        expect(field).to_have_value("50")
        expect(page.locator(".brand")).to_have_attribute("href", "/environments/production/flags")
        browser.close()


def test_long_flag_key_stays_clear_of_controls(server, admin):
    key = "checkout_" + "a" * 55
    response = admin.post("/api/environments/development/flags", json={"key": key, "mode": "enabled"})
    assert response.status_code == 201
    with sync_playwright() as playwright:
        browser = launch_browser(playwright)
        page = browser.new_page()
        login(page, server)
        row = page.locator(".flag-row")
        for width in (1360, 900, 768, 390):
            page.set_viewport_size({"width": width, "height": 900})
            assert page.evaluate("document.documentElement.scrollWidth <= innerWidth")
            key_box = row.locator(".flag-name").bounding_box()
            mode_box = row.locator(".mode-label").bounding_box()
            actions_box = row.locator(".row-actions").bounding_box()
            assert key_box["x"] + key_box["width"] <= actions_box["x"]
            assert (key_box["x"] + key_box["width"] <= mode_box["x"]
                    or key_box["y"] + key_box["height"] <= mode_box["y"])
        browser.close()


def test_pending_creation_cannot_close_or_replace_draft(server, admin):
    with sync_playwright() as playwright:
        browser = launch_browser(playwright)
        page = browser.new_page()
        login(page, server)
        pending = []
        page.get_by_role("button", name="Create flag", exact=True).click()
        modal = page.get_by_role("dialog", name="Create a flag")
        modal.get_by_label("Flag key", exact=True).fill("pending_flag")
        page.route("**/api/environments/development/flags", lambda route: pending.append(route))
        modal.get_by_role("button", name="Create flag", exact=True).click()
        expect(modal.get_by_label("Flag key", exact=True)).to_be_disabled()
        expect(modal.get_by_role("button", name="Close flag dialog", exact=True)).to_be_disabled()
        expect(modal.get_by_role("button", name="Cancel", exact=True)).to_be_disabled()
        page.keyboard.press("Escape")
        expect(modal).to_be_visible()
        expect(page.get_by_role("dialog", name="Discard your changes?")).to_be_hidden()
        assert len(pending) == 1
        pending.pop().continue_()
        expect(modal).to_be_hidden()
        expect(page.locator('.flag-row[data-key="pending_flag"]')).to_be_visible()
        page.get_by_role("button", name="Create flag", exact=True).click()
        expect(modal.get_by_role("button", name="Close flag dialog", exact=True)).to_be_enabled()
        expect(modal.get_by_label("Flag key", exact=True)).to_have_value("")
        browser.close()


def test_utilitarian_headers_select_clearance_and_sticky_navigation(server, admin):
    with sync_playwright() as playwright:
        browser = launch_browser(playwright)
        page = browser.new_page()
        login(page, server, environment="production")
        for width in (1280, 390, 320):
            page.set_viewport_size({"width": width, "height": 900})
            flag_button = page.locator("#create-flag").bounding_box()
            page.get_by_role("tab", name="Apps & Keys").click()
            key_button = page.locator("#create-app").bounding_box()
            assert flag_button == key_button
            expect(page.get_by_text("API Keys can read feature flags and rules. Use the UI to make changes.", exact=True)).to_be_visible()
            page.get_by_role("tab", name="Docs").click()
            expect(page.locator(".docs-guide")).to_have_count(0)
            expect(page.locator("footer")).to_have_count(0)
            expect(page.get_by_text("Default project", exact=True)).to_have_count(0)
            for dropdown in page.get_by_role("combobox").all():
                geometry = dropdown.evaluate("""e => {
                    const style = getComputedStyle(e);
                    const arrow = getComputedStyle(e.parentElement, '::after');
                    return { padding: parseFloat(style.paddingRight), inset: parseFloat(arrow.right),
                             events: arrow.pointerEvents };
                }""")
                assert geometry == {"padding": 36, "inset": 14, "events": "none"}
            assert page.evaluate("document.documentElement.scrollWidth <= innerWidth")
            expect(page.locator(".brand-mark")).to_be_visible()
            header = page.locator(".site-header").bounding_box()
            tabs = page.get_by_role("tablist").bounding_box()
            page.evaluate("window.scrollTo(0, document.body.scrollHeight)")
            page.wait_for_function("scrollY > 0")
            assert page.locator(".site-header").bounding_box() == header
            assert page.get_by_role("tablist").bounding_box() == tabs
            page.get_by_role("tab", name="Feature flags").click()
        page.get_by_role("button", name="Create flag", exact=True).click()
        modal = page.get_by_role("dialog", name="Create a flag")
        mode = modal.get_by_label("Rollout mode for new flag", exact=True)
        mode.focus()
        page.keyboard.press("c")
        expect(mode).to_have_attribute("value", "disabled")
        page.keyboard.press("Enter")
        expect(mode).to_have_attribute("value", "custom")
        for dropdown in modal.get_by_role("combobox").all():
            assert dropdown.evaluate("e => parseFloat(getComputedStyle(e).paddingRight)") == 36
            assert dropdown.evaluate("e => getComputedStyle(e.parentElement, '::after').pointerEvents") == "none"
        page.on("dialog", lambda dialog: dialog.accept())
        browser.close()


def test_key_modal_dirty_and_pending_creation_safeguards(server, admin):
    with sync_playwright() as playwright:
        browser = launch_browser(playwright)
        page = browser.new_page()
        login(page, server, screen="apps")
        open_key(page)
        modal = page.get_by_role("dialog", name="Generate API Key", exact=True)
        expect(modal.get_by_label("Key name", exact=True)).to_be_focused()
        expect(modal.get_by_text("Generate API Key for Test application in development.")).to_be_visible()
        modal.get_by_label("Key name", exact=True).fill("Checkout service")
        page.keyboard.press("Escape")
        expect(page.get_by_role("dialog", name="Discard your changes?")).to_be_visible()
        page.get_by_role("button", name="Keep editing").click()
        expect(modal.get_by_label("Key name", exact=True)).to_have_value("Checkout service")
        pending = []
        page.route("**/api/environments/development/apps/*/keys", lambda route: pending.append(route))
        modal.get_by_role("button", name="Generate key", exact=True).click()
        expect(modal.get_by_label("Key name", exact=True)).to_be_disabled()
        expect(modal.get_by_role("button", name="Close key dialog", exact=True)).to_be_disabled()
        page.keyboard.press("Escape")
        expect(modal).to_be_visible()
        assert len(pending) == 1
        pending.pop().continue_()
        expect(modal).to_be_hidden()
        expect(page.get_by_label("API Key for Checkout service", exact=True)).to_have_attribute("type", "text")
        open_key(page)
        expect(modal.get_by_label("Key name", exact=True)).to_have_value("")
        modal.get_by_role("button", name="Cancel", exact=True).click()
        expect(modal).to_be_hidden()
        expect(page.get_by_role("dialog", name="Discard your changes?")).to_be_hidden()
        browser.close()


def test_success_feedback_never_moves_existing_content(server, admin):
    def geometry(page, selector):
        return page.locator(selector).evaluate_all("items => items.map(e => e.getBoundingClientRect().toJSON())")

    with sync_playwright() as playwright:
        browser = launch_browser(playwright)
        page = browser.new_page(permissions=["clipboard-read", "clipboard-write"])
        login(page, server)
        create_flag(page, "checkout", "percentage", 50)
        page.get_by_role("tab", name="Apps & Keys").click()
        create_key(page, "Worker")
        create_key(page, "Other worker")
        page.clock.install()
        artifacts = ROOT / "test-results"
        artifacts.mkdir(exist_ok=True)
        for width in (1280, 390, 320):
            page.set_viewport_size({"width": width, "height": 900})
            key_elements = ".workspace-heading, .key-row, .key-row input, .key-row button, #apps-screen > .hint"
            before = geometry(page, key_elements)
            row = page.locator(".key-row").first
            row.get_by_role("button", name=re.compile("^Copy API Key ")).click()
            expect(row.get_by_role("status")).to_have_text("Copied")
            assert geometry(page, key_elements) == before
            page.screenshot(path=str(artifacts / f"stable-key-feedback-{width}.png"), full_page=True)
            page.clock.fast_forward(3000)
            expect(page.get_by_role("status")).to_be_hidden()
            assert geometry(page, key_elements) == before
            row.locator('input[name="name"]').fill(f"Worker {width}")
            row.get_by_role("button", name=re.compile("^Save name for ")).click()
            expect(row.get_by_role("status")).to_have_text("Saved")
            assert geometry(page, key_elements) == before

            page.get_by_role("tab", name="Docs").click()
            expect(page.get_by_role("button", name="Copy code")).to_be_enabled()
            doc_elements = ".workspace-heading, .code-panel, .code-heading, .code-heading > span, .code-heading label, #copy-example, .code-example"
            before = geometry(page, doc_elements)
            page.get_by_role("button", name="Copy code").click()
            expect(page.get_by_role("status")).to_have_text("Copied")
            assert geometry(page, doc_elements) == before
            assert page.evaluate("document.documentElement.scrollWidth <= innerWidth")
            page.screenshot(path=str(artifacts / f"stable-docs-feedback-{width}.png"), full_page=True)
            page.clock.fast_forward(3000)
            expect(page.get_by_role("status")).to_be_hidden()
            assert geometry(page, doc_elements) == before

            page.get_by_role("tab", name="Feature flags").click()
            page.get_by_label("Percentage for checkout").fill(str(width % 100))
            flag_elements = ".workspace-heading, .flag-row, .flag-heading, .flag-row .dropdown, .flag-row input, .flag-row button"
            before = geometry(page, flag_elements)
            page.get_by_role("button", name="Save checkout", exact=True).click()
            confirm_flag(page, "checkout")
            expect(page.get_by_role("status")).to_have_text("Saved")
            assert geometry(page, flag_elements) == before
            page.clock.fast_forward(3000)
            expect(page.get_by_role("status")).to_be_hidden()
            assert geometry(page, flag_elements) == before
            page.get_by_role("tab", name="Apps & Keys").click()
        browser.close()


def test_docs_highlighting_preserves_literal_source_and_never_renders_html(server, admin):
    source = '''# Literal example containing HTML-like data
API_KEY = "<img src=x onerror=alert(1)>"
async def main():
    count = 18
    print(f"Value: {count}")
'''
    with sync_playwright() as playwright:
        browser = launch_browser(playwright)
        page = browser.new_page(permissions=["clipboard-read", "clipboard-write"])
        page.route("**/docs/python.py", lambda route: route.fulfill(status=200, content_type="text/plain", body=source))
        login(page, server, screen="docs")
        code = page.locator("#docs-code")
        expect(code).to_have_text(source)
        assert code.text_content() == source
        assert code.locator("img, script").count() == 0
        for kind in ("comment", "string", "keyword", "number"):
            expect(code.locator(f".token-{kind}").first).to_be_visible()
        colors = code.locator("span").evaluate_all("items => [...new Set(items.map(e => getComputedStyle(e).color))]")
        assert len(colors) >= 3
        page.get_by_role("button", name="Copy code", exact=True).click()
        expect(page.get_by_role("status")).to_have_text("Copied")
        assert page.evaluate("navigator.clipboard.readText()") == source
        browser.close()


def test_custom_dropdown_keyboard_commit_cancel_and_modal_escape(server, admin):
    with sync_playwright() as playwright:
        browser = launch_browser(playwright)
        page = browser.new_page()
        login(page, server)
        page.get_by_role("button", name="Create flag", exact=True).click()
        modal = page.get_by_role("dialog", name="Create a flag")
        mode = modal.get_by_role("combobox", name="Rollout mode for new flag", exact=True)
        popup = page.locator("#dropdown-options")
        expect(mode).to_have_attribute("type", "button")
        expect(mode).to_have_attribute("value", "disabled")
        mode.click()
        expect(popup).to_be_visible()
        expect(mode).to_have_attribute("aria-expanded", "true")
        expect(popup.locator('[data-value="disabled"]')).to_have_attribute("aria-selected", "true")
        page.keyboard.press("Home")
        expect_active_option(page, mode, "enabled")
        page.keyboard.press("ArrowDown")
        expect_active_option(page, mode, "percentage")
        expect(mode).to_have_attribute("value", "disabled")
        expect(popup.locator('[data-value="disabled"]')).to_have_attribute("aria-selected", "true")
        page.keyboard.press("Escape")
        expect(popup).to_be_hidden()
        expect(mode).to_have_attribute("aria-expanded", "false")
        expect(mode).to_have_attribute("value", "disabled")
        expect(modal).to_be_visible()
        expect(page.get_by_role("dialog", name="Discard your changes?")).to_be_hidden()
        expect(mode).to_be_focused()

        page.keyboard.press("Enter")
        expect(popup).to_be_visible()
        page.keyboard.press("End")
        expect_active_option(page, mode, "disabled")
        page.keyboard.press("ArrowUp")
        expect_active_option(page, mode, "custom")
        page.keyboard.press("Space")
        expect(popup).to_be_hidden()
        expect(mode).to_have_attribute("value", "custom")
        expect(modal.get_by_role("combobox", name="Match action for new flag")).to_be_visible()

        mode.focus()
        page.keyboard.press("p")
        expect_active_option(page, mode, "percentage")
        expect(mode).to_have_attribute("value", "custom")
        page.keyboard.press("Enter")
        expect(mode).to_have_attribute("value", "percentage")
        expect(modal.get_by_label("Percentage for new flag", exact=True)).to_be_visible()

        mode.click()
        page.keyboard.press("Home")
        page.keyboard.press("Tab")
        expect(popup).to_be_hidden()
        expect(mode).to_have_attribute("value", "enabled")
        expect(mode).not_to_be_focused()
        expect(modal).to_be_visible()

        mode.click()
        page.keyboard.press("End")
        modal.get_by_label("Flag key", exact=True).click()
        expect(popup).to_be_hidden()
        expect(mode).to_have_attribute("value", "enabled")
        expect(modal.get_by_label("Flag key", exact=True)).to_be_focused()
        mode.click()
        page.keyboard.press("End")
        modal.get_by_label("Flag key", exact=True).focus()
        expect(popup).to_be_hidden()
        expect(mode).to_have_attribute("value", "enabled")
        page.on("dialog", lambda dialog: dialog.accept())
        browser.close()


def test_environment_dropdown_only_navigates_after_commit(server, admin):
    with sync_playwright() as playwright:
        browser = launch_browser(playwright)
        page = browser.new_page()
        login(page, server)
        environment = page.get_by_role("combobox", name="Environment", exact=True)
        page.evaluate("window.environmentNavigationMarker = true")
        environment.click()
        page.keyboard.press("End")
        expect_active_option(page, environment, "production")
        expect(environment).to_have_attribute("value", "development")
        expect(page).to_have_url(f"{server.url}/environments/development/flags")
        page.keyboard.press("Escape")
        assert page.evaluate("window.environmentNavigationMarker")
        environment.focus()
        page.keyboard.press("p")
        expect_active_option(page, environment, "production")
        expect(environment).to_have_attribute("value", "development")
        page.keyboard.press("Tab")
        expect(page).to_have_url(f"{server.url}/environments/production/flags")
        expect(environment).to_have_attribute("value", "production")
        assert page.evaluate("window.environmentNavigationMarker === undefined")
        browser.close()


def test_dropdown_popup_fits_viewport_without_layout_shift_and_cancels_on_move(server, admin):
    def fits_viewport(page, popup):
        box = popup.bounding_box()
        viewport = page.viewport_size
        assert box is not None
        assert box["x"] >= 0 and box["y"] >= 0
        assert box["x"] + box["width"] <= viewport["width"]
        assert box["y"] + box["height"] <= viewport["height"]

    with sync_playwright() as playwright:
        browser = launch_browser(playwright)
        page = browser.new_page()
        login(page, server)
        for width in (1280, 390, 320):
            page.set_viewport_size({"width": width, "height": 640})
            environment = page.get_by_role("combobox", name="Environment", exact=True)
            popup = page.locator("#dropdown-options")
            header_box = page.locator(".site-header").bounding_box()
            trigger_box = environment.bounding_box()
            environment.click()
            expect(popup).to_be_visible()
            fits_viewport(page, popup)
            assert page.locator(".site-header").bounding_box() == header_box
            assert environment.bounding_box() == trigger_box
            page.keyboard.press("Escape")
            page.get_by_role("button", name="Create flag", exact=True).click()
            modal = page.get_by_role("dialog", name="Create a flag")
            choose(page, modal.get_by_role("combobox", name="Rollout mode for new flag"), "custom")
            operator = modal.get_by_role("combobox", name="Operator", exact=True)
            operator.scroll_into_view_if_needed()
            before = operator.bounding_box()
            modal_box = modal.bounding_box()
            operator.click()
            expect(popup).to_be_visible()
            expect(popup.get_by_role("option")).to_have_count(11)
            assert popup.evaluate("e => e.closest('dialog').id") == "flag-dialog"
            assert operator.bounding_box() == before
            assert modal.bounding_box() == modal_box
            fits_viewport(page, popup)
            page.keyboard.press("End")
            expect_active_option(page, operator, "is")
            expect(popup.locator('[data-value="is"]')).to_be_in_viewport(ratio=0.9)
            expect(operator).to_have_attribute("value", "matches")
            page.set_viewport_size({"width": width, "height": 630})
            expect(popup).to_be_hidden()
            expect(operator).to_have_attribute("value", "matches")
            page.set_viewport_size({"width": width, "height": 640})
            operator.click()
            expect(popup).to_be_visible()
            modal.evaluate("e => { e.scrollTop = e.scrollTop ? 0 : e.scrollHeight; }")
            expect(popup).to_be_hidden()
            expect(operator).to_have_attribute("value", "matches")
            choose(page, modal.get_by_role("combobox", name="Rollout mode for new flag"), "disabled")
            modal.get_by_role("button", name="Cancel", exact=True).click()
            expect(modal).to_be_hidden()
            assert page.evaluate("document.documentElement.scrollWidth <= innerWidth")
        browser.close()


def test_app_selection_typed_confirmation_revoked_history_and_audits(server, admin):
    with sync_playwright() as playwright:
        browser = launch_browser(playwright)
        page = browser.new_page(viewport={"width": 1280, "height": 900})
        page.add_init_script("Object.defineProperty(navigator, 'clipboard', {value: {writeText: async value => { window.partlyCopied = value; }}})")
        errors = []
        page.on("pageerror", lambda error: errors.append(str(error)))
        login(page, server)
        create_flag(page, "shared", "enabled")
        create_flag(page, "unused", "disabled")
        choose(page, page.get_by_label("Rollout mode for unused", exact=True), "enabled")
        page.get_by_role("button", name="Save unused", exact=True).click()
        modal = page.locator("#confirm-flag-dialog")
        expect(modal.get_by_text("No applications use this flag.", exact=True)).to_be_visible()
        expect(modal.locator("#confirm-flag-submit")).to_be_disabled()
        modal.locator("#confirm-flag-input").fill("production/unused")
        expect(modal.locator("#confirm-flag-submit")).to_be_disabled()
        modal.get_by_role("button", name="Keep editing", exact=True).click()
        expect(page.get_by_role("button", name="Save unused", exact=True)).to_be_enabled()
        page.get_by_role("button", name="Save unused", exact=True).click()
        confirm_flag(page, "unused")
        page.get_by_role("tab", name="Apps & Keys", exact=True).click()
        for name in ("Billing", "Storefront"):
            page.get_by_role("button", name="Create app", exact=True).click()
            create = page.get_by_role("dialog", name="Create app", exact=True)
            create.get_by_label("Application name", exact=True).fill(name)
            expect(create.get_by_role("checkbox", name="shared", exact=True)).not_to_be_checked()
            create.get_by_role("checkbox", name="shared", exact=True).check()
            create.get_by_role("button", name="Create app", exact=True).click()
            expect(create).to_be_hidden()
        billing = page.locator(".app-row").filter(has=page.get_by_role("heading", name="Billing", exact=True))
        application_id = billing.get_attribute("data-app-id")
        assert re.fullmatch(r"app_[0-9a-f]{20}", application_id)
        billing.get_by_role("button", name="Copy ID", exact=True).click()
        assert page.evaluate("window.partlyCopied") == application_id
        billing.locator(".app-settings summary").click()
        billing.get_by_role("checkbox", name="unused", exact=True).check()
        page.get_by_role("tab", name="Docs", exact=True).click()
        expect(page.get_by_role("dialog", name="Discard your changes?")).to_be_visible()
        page.get_by_role("button", name="Keep editing", exact=True).click()
        billing.get_by_role("button", name="Save app", exact=True).click()
        expect(billing.get_by_role("button", name="Save app", exact=True)).to_be_disabled()
        page.get_by_role("tab", name="Feature flags", exact=True).click()
        choose(page, page.get_by_label("Rollout mode for shared", exact=True), "disabled")
        page.get_by_role("button", name="Save shared", exact=True).click()
        expect(modal.locator("li")).to_have_text(["Billing", "Storefront"])
        modal.locator("#confirm-flag-input").fill("development/Shared")
        expect(modal.locator("#confirm-flag-submit")).to_be_disabled()
        artifacts = ROOT / "test-results"
        artifacts.mkdir(exist_ok=True)
        page.screenshot(path=str(artifacts / "flag-confirmation.png"), full_page=True)
        confirm_flag(page, "shared")
        page.get_by_role("tab", name="Audits", exact=True).click()
        latest = page.locator(".audit-row").first
        expect(latest.locator(".type-pill")).to_have_text("Flag")
        latest.locator("summary").click()
        expect(latest.locator(".audit-detail")).to_contain_text("Billing, Storefront")
        expect(latest.locator(".audit-detail")).to_contain_text('"enabled"')
        expect(latest.locator(".audit-detail")).to_contain_text('"disabled"')
        choose(page, page.get_by_label("Filter by action", exact=True), "updated")
        choose(page, page.get_by_label("Filter by application", exact=True), application_id)
        expect(page.locator(".audit-row")).to_have_count(2)
        page.screenshot(path=str(artifacts / "audits-desktop.png"), full_page=True)
        page.get_by_role("tab", name="Apps & Keys", exact=True).click()
        secret_field = billing.get_by_label("API Key for Default", exact=True)
        mask = secret_field.input_value()
        assert "•" in mask
        billing.get_by_role("button", name="Reveal", exact=True).click()
        secret = secret_field.input_value()
        assert secret.startswith(mask[:7]) and secret.endswith(mask[-4:])
        billing.get_by_role("button", name="Hide", exact=True).click()
        expect(secret_field).to_have_value(mask)
        page.on("dialog", lambda dialog: dialog.accept())
        billing.get_by_role("button", name="Revoke", exact=True).click()
        expect(billing.get_by_text("Revoked keys (1)", exact=True)).to_be_visible()
        billing.locator(".revoked-keys summary").click()
        expect(billing.locator(".revoked-key code")).to_have_text(mask)
        assert billing.locator(".revoked-key button, .revoked-key input").count() == 0
        response = page.request.get(f"{server.url}/api/environments/development/apps").json()
        retained = next(app for app in response["applications"] if app["id"] == application_id)["keys"][0]
        assert retained["secret"] is None and retained["revoked_at"]
        page.screenshot(path=str(artifacts / "apps-desktop.png"), full_page=True)
        page.set_viewport_size({"width": 390, "height": 844})
        assert page.evaluate("document.documentElement.scrollWidth <= innerWidth")
        page.screenshot(path=str(artifacts / "apps-mobile.png"), full_page=True)
        assert not errors
        browser.close()
