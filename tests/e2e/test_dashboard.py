import re

from playwright.sync_api import sync_playwright, expect

from conftest import PASSWORD, ROOT


def login(page, server, screen="flags", environment="development"):
    page.goto(f"{server.url}/environments/{environment}/{screen}")
    page.get_by_label("Username", exact=True).fill("admin")
    page.get_by_label("Password", exact=True).fill(PASSWORD)
    page.get_by_role("button", name="Sign in", exact=True).click()
    expect(page.locator("#workspace")).to_be_visible()


def create_flag(page, key, mode="disabled", percentage=None):
    page.get_by_role("button", name="+ Create flag", exact=True).click()
    dialog = page.get_by_role("dialog", name="Create a flag")
    dialog.get_by_label("Flag key", exact=True).fill(key)
    dialog.get_by_label("Rollout mode for new flag", exact=True).select_option(mode)
    if percentage is not None:
        dialog.get_by_label("Percentage for new flag", exact=True).fill(str(percentage))
    dialog.get_by_role("button", name="Create flag", exact=True).click()
    expect(dialog).to_be_hidden()
    expect(page.locator(f'.flag-row[data-key="{key}"]')).to_be_visible()


def test_dashboard_setup_flags_keys_and_restart(server):
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch()
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
        expect(page.get_by_role("status")).to_contain_text("Saved new_checkout")
        expect(page.get_by_role("tab")).to_have_text(["Feature flags", "Key Management", "Docs"])
        artifacts = ROOT / "test-results"
        artifacts.mkdir(exist_ok=True)
        page.screenshot(path=str(artifacts / "dashboard-desktop.png"), full_page=True)
        page.get_by_role("tab", name="Key Management").click()
        expect(page).to_have_url(f"{server.url}/environments/development/keys")
        page.get_by_label("Key name", exact=True).fill("Checkout service")
        page.get_by_role("button", name="Generate key").click()
        field = page.locator(".key-controls input")
        expect(field).to_have_attribute("type", "password")
        expect(field).to_have_value(re.compile(r"pk_[A-Za-z0-9]{24}"))
        requests = []
        page.on("request", lambda request: requests.append(request.url))
        copy = page.get_by_role("button", name=re.compile("^Copy SDK key "))
        copy.click()
        expect(page.get_by_role("status")).to_have_text("SDK key copied.")
        expect(copy).to_have_text("Copied")
        secret = page.evaluate("navigator.clipboard.readText()")
        assert re.fullmatch(r"pk_[A-Za-z0-9]{24}", secret)
        expect(field).to_have_value(secret)
        expect(field).to_have_attribute("type", "password")
        page.get_by_role("button", name="Reveal", exact=True).click()
        expect(field).to_have_attribute("type", "text")
        page.get_by_role("button", name="Hide", exact=True).click()
        expect(field).to_have_attribute("type", "password")
        assert requests == []
        assert page.locator(".key-row code").count() == 0
        page.get_by_label("Name for Checkout service", exact=True).fill("Checkout worker")
        page.get_by_role("button", name="Save name for Checkout service").click()
        expect(page.get_by_label("Name for Checkout worker", exact=True)).to_have_value("Checkout worker")
        server.stop()
        server.start()
        page.reload()
        page.get_by_label("Username", exact=True).fill("admin")
        page.get_by_label("Password", exact=True).fill(PASSWORD)
        page.get_by_role("button", name="Sign in", exact=True).click()
        expect(page.get_by_label("Name for Checkout worker", exact=True)).to_have_value("Checkout worker")
        expect(field).to_have_attribute("type", "password")
        expect(field).to_have_value(secret)
        copy.focus()
        page.keyboard.press("Enter")
        expect(page.get_by_role("status")).to_have_text("SDK key copied.")
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
        expect(field).to_have_attribute("type", "password")
        page.evaluate("delete navigator.clipboard.writeText")
        page.get_by_label("Environment", exact=True).select_option("production")
        expect(page).to_have_url(f"{server.url}/environments/production/keys")
        expect(page.get_by_text("Generate a key to connect your Python application.")).to_be_visible()
        page.get_by_role("tab", name="Feature flags").click()
        expect(page.get_by_text("No flags in this environment")).to_be_visible()
        page.get_by_label("Environment", exact=True).select_option("development")
        expect(page.get_by_label("Percentage for new_checkout")).to_have_value("50")
        page.screenshot(path=str(artifacts / "dashboard-mobile.png"), full_page=True)
        page.on("dialog", lambda dialog: dialog.accept())
        page.get_by_role("button", name="Delete new_checkout").click()
        expect(page.get_by_text("No flags in this environment")).to_be_visible()
        page.get_by_role("tab", name="Key Management").click()
        page.get_by_role("button", name="Revoke", exact=True).click()
        expect(page.get_by_text("Generate a key to connect your Python application.")).to_be_visible()
        page.get_by_role("button", name="Sign out").click()
        expect(page.get_by_role("button", name="Sign in", exact=True)).to_be_visible()
        expect(page.get_by_role("tablist")).to_be_hidden()
        assert page.locator(".key-controls input").count() == 0
        assert not errors
        browser.close()


def test_docs_navigation_and_complete_example(server, admin):
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch()
        page = browser.new_page(viewport={"width": 1280, "height": 1000}, permissions=["clipboard-read", "clipboard-write"])
        login(page, server)
        flags = page.get_by_role("tab", name="Feature flags")
        keys = page.get_by_role("tab", name="Key Management")
        docs = page.get_by_role("tab", name="Docs")
        flags.focus()
        page.keyboard.press("ArrowRight")
        expect(keys).to_be_focused()
        expect(keys).to_have_attribute("aria-selected", "true")
        page.keyboard.press("End")
        expect(docs).to_be_focused()
        expect(docs).to_have_attribute("aria-selected", "true")
        expect(page.get_by_label("Language", exact=True)).to_have_value("python")
        expect(page.get_by_label("Language", exact=True).locator("option")).to_have_text(["Python"])
        expect(page.get_by_role("button", name="Copy code")).to_be_enabled()
        code = (ROOT / "examples/python/quickstart.py").read_text()
        assert page.locator("#docs-code").text_content() == code
        page.get_by_role("button", name="Copy code").click()
        expect(page.get_by_role("status")).to_have_text("Python example copied.")
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
        browser = playwright.chromium.launch()
        page = browser.new_page(viewport={"width": 1280, "height": 1000})
        login(page, server)
        create_flag(page, "other_flag", "percentage", 50)
        page.get_by_label("Percentage for other_flag").fill("75")
        requests = []
        page.on("request", lambda request: requests.append((request.method, request.url)))
        page.get_by_role("button", name="+ Create flag", exact=True).click()
        modal = page.get_by_role("dialog", name="Create a flag")
        expect(modal.get_by_text("Create flag in development.", exact=True)).to_be_visible()
        assert modal.get_by_label("Environment", exact=True).count() == 0
        expect(modal.get_by_label("Flag key", exact=True)).to_be_focused()
        modal.get_by_label("Flag key", exact=True).fill("eligible")
        modal.get_by_label("Rollout mode for new flag").select_option("custom")
        first = modal.locator(".condition").first
        first.get_by_label("Context key", exact=True).fill("age")
        for op in ("le", "lt", "ge", "gt", "eq"):
            first.get_by_label("Operator", exact=True).select_option(op)
            expect(first.get_by_label("Comparison value", exact=True)).to_have_attribute("type", "number")
        expect(first.get_by_text(re.compile("ContextTypeError"))).to_be_visible()
        first.get_by_label("Comparison value", exact=True).fill("9007199254740993")
        modal.get_by_role("button", name="Create flag", exact=True).click()
        expect(modal).to_be_visible()
        assert first.get_by_label("Comparison value", exact=True).evaluate("e => !e.checkValidity()")
        first.get_by_label("Comparison value", exact=True).fill("-18.5")
        for op in ("prefix", "suffix", "contains", "matches", "regex"):
            first.get_by_label("Operator", exact=True).select_option(op)
            expect(first.get_by_label("Comparison value", exact=True)).to_have_attribute("type", "text")
        expect(first.get_by_label("Comparison value", exact=True)).to_have_value("")
        first.get_by_label("Operator", exact=True).select_option("ge")
        first.get_by_label("Comparison value", exact=True).fill("18")
        modal.get_by_role("button", name="+ AND condition", exact=True).click()
        second = modal.locator(".condition").nth(1)
        second.get_by_label("Context key", exact=True).fill("verified")
        second.get_by_label("Operator", exact=True).select_option("is")
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
        row.get_by_label("Match action for eligible").select_option("disable")
        page.get_by_role("button", name="Save eligible", exact=True).click()
        expect(page.get_by_role("status")).to_contain_text("Saved eligible")
        expect(page.get_by_label("Percentage for other_flag")).to_have_value("75")
        expect(page.get_by_role("button", name="Save other_flag", exact=True)).to_be_enabled()
        assert not [request for request in requests if request[0] == "GET"]
        saved = admin.get("/api/environments/development/flags").json()["eligible"]
        assert saved == {"mode": "custom", "action": "disable", "groups": [
            [{"k": "age", "op": "ge", "v": 18}, {"k": "verified", "op": "is", "v": True}],
            [{"k": "plan", "op": "matches", "v": "pro"}],
        ]}
        # Duplicate creation stays in the modal with its draft intact.
        page.get_by_role("button", name="+ Create flag", exact=True).click()
        modal.get_by_label("Flag key", exact=True).fill("eligible")
        modal.get_by_role("button", name="Create flag", exact=True).click()
        expect(modal.get_by_role("alert")).to_be_visible()
        expect(modal.get_by_label("Flag key", exact=True)).to_have_value("eligible")
        page.on("dialog", lambda dialog: dialog.accept())
        browser.close()


def test_unsaved_navigation_reload_and_modal_protection(server, admin):
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch()
        page = browser.new_page()
        login(page, server)
        create_flag(page, "checkout", "percentage", 50)
        field = page.get_by_label("Percentage for checkout")
        field.fill("")  # Invalid/incomplete drafts still need protection.
        page.get_by_label("Environment", exact=True).select_option("production")
        discard = page.get_by_role("dialog", name="Discard your changes?")
        expect(discard).to_be_visible()
        page.get_by_role("button", name="Keep editing").click()
        expect(field).to_have_value("")
        expect(page.get_by_label("Environment", exact=True)).to_have_value("development")
        field.fill("50")
        expect(page.get_by_role("button", name="Save checkout", exact=True)).to_be_disabled()
        # Returning to the baseline removes the warning entirely.
        page.evaluate("window.reloadMarker = true")
        page.get_by_label("Environment", exact=True).select_option("production")
        expect(page).to_have_url(f"{server.url}/environments/production/flags")
        assert page.evaluate("window.reloadMarker === undefined")
        page.get_by_label("Environment", exact=True).select_option("development")
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
        page.get_by_role("button", name="+ Create flag", exact=True).click()
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
        page.get_by_role("tab", name="Key Management").click()
        page.get_by_label("Key name", exact=True).fill("unsaved application")
        page.get_by_role("tab", name="Docs").click()
        expect(discard).to_be_visible()
        page.get_by_role("button", name="Keep editing").click()
        expect(page.get_by_label("Key name", exact=True)).to_have_value("unsaved application")
        page.on("dialog", lambda dialog: dialog.accept())
        browser.close()


def test_failed_save_preserves_draft_and_key_rename_warns(server, admin):
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch()
        page = browser.new_page()
        login(page, server)
        create_flag(page, "checkout", "percentage", 50)
        page.get_by_label("Percentage for checkout").fill("75")
        page.route("**/api/environments/development/flags/checkout", lambda route: route.fulfill(status=500, json={"error": "Temporary failure"}))
        page.get_by_role("button", name="Save checkout", exact=True).click()
        expect(page.get_by_role("alert")).to_have_text("Temporary failure")
        expect(page.get_by_label("Percentage for checkout")).to_have_value("75")
        expect(page.get_by_role("button", name="Save checkout", exact=True)).to_be_enabled()
        page.get_by_role("tab", name="Key Management").click()
        page.get_by_role("button", name="Discard changes").click()
        page.get_by_label("Key name", exact=True).fill("Worker")
        page.get_by_role("button", name="Generate key").click()
        name = page.get_by_label("Name for Worker", exact=True)
        name.fill("Renamed worker")
        page.get_by_label("Environment", exact=True).select_option("production")
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
        browser = playwright.chromium.launch()
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
        expect(page.get_by_role("button", name="Save search", exact=True)).to_be_enabled()
        page.on("dialog", lambda dialog: dialog.accept())
        browser.close()


def test_long_flag_key_stays_clear_of_controls(server, admin):
    key = "checkout_" + "a" * 55
    response = admin.post("/api/environments/development/flags", json={"key": key, "mode": "enabled"})
    assert response.status_code == 201
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch()
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
        browser = playwright.chromium.launch()
        page = browser.new_page()
        login(page, server)
        pending = []
        page.get_by_role("button", name="+ Create flag", exact=True).click()
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
        page.get_by_role("button", name="+ Create flag", exact=True).click()
        expect(modal.get_by_role("button", name="Close flag dialog", exact=True)).to_be_enabled()
        expect(modal.get_by_label("Flag key", exact=True)).to_have_value("")
        browser.close()
