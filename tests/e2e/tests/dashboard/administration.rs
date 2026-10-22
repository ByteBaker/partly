// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;

#[tokio::test]
async fn dashboard_setup_flags_keys_and_restart() -> Result {
    let mut server = Server::new()?;
    server.start().await?;
    run("setup-restart",async |ui| {
        let page = &ui.page;
        // Chromium also exercises the system clipboard; the other engines lack this permission.
        if std::env::var("PARTLY_BROWSER").unwrap_or_default()!="chromium" && std::env::var("PARTLY_BROWSER").is_ok() {
            page.add_init_script("Object.defineProperty(navigator, 'clipboard', {value: {writeText: async text => { window.partlyCopied = text; }}})").await?;
        }
        page.goto(&server.url,None).await?;
        expect(page.get_by_role(Button,named("Create admin account"))).to_be_visible().await?;
        expect(page.locator("#password-help")).to_have_text("Use at least 12 characters.").await?;
        expect(page.locator("#password")).to_have_attribute("aria-describedby","password-help").await?;
        assert!(!server.db.exists());
        page.locator("#auth").get_by_label("Username",true).fill("admin",None).await?;
        page.locator("#auth").get_by_label("Password",true).fill(PASSWORD,None).await?;
        page.get_by_role(Button,named("Create admin account")).click(None).await?;
        expect_page(page).to_have_url(&format!("{}/environments/development/flags",server.url)).await?;
        assert!(server.db.exists());
        create_flag(page,"new_checkout","percentage",Some(25)).await?;
        page.get_by_label("Percentage for new_checkout",true).fill("50",None).await?;
        page.get_by_role(Button,named("Save new_checkout")).click(None).await?;
        confirm_flag(page,"new_checkout").await?;
        expect(page.get_by_role(Status,None)).to_contain_text("Saved").await?;
        texts(page.get_by_role(Tab,None),&["Feature flags","Apps & Keys","Audits","Docs","Users"]).await?;
        page.get_by_role(Tab,named("Apps & Keys")).click(None).await?;
        expect_page(page).to_have_url(&format!("{}/environments/development/apps",server.url)).await?;
        let heading = geometry(page,".workspace-heading").await?;
        create_key(page,"Checkout service").await?;
        expect(page.locator(".key-row").get_by_role(Status,None)).to_have_text("Saved").await?;
        assert_eq!(geometry(page,".workspace-heading").await?,heading);
        let field = page.locator(".key-controls input");
        expect(field.clone()).to_have_attribute("type","text").await?;
        expect(field.clone()).to_have_value_regex("^pk_[A-Za-z0-9]{4}•+[A-Za-z0-9]{4}$").await?;
        ui.requests.lock().unwrap().clear();
        let copy = page.get_by_role(Button,containing("Copy API Key "));
        copy.click(None).await?;
        expect(page.get_by_role(Status,None)).to_have_text("Copied").await?;
        expect(page.locator(".key-row").get_by_role(Status,None)).to_have_text("Copied").await?;
        assert_eq!(geometry(page,".workspace-heading").await?,heading);
        expect(copy.clone()).to_have_text("Copy").await?;
        let secret = js(page,"async () => window.partlyCopied ?? await navigator.clipboard.readText()").await?;
        let secret = text(&secret).to_owned();
        assert!(secret.starts_with("pk_") && secret.len()==27 && secret[3..].chars().all(|c|c.is_ascii_alphanumeric()));
        let mask = format!("{}{}{}",&secret[..7],"•".repeat(12),&secret[secret.len()-4..]);
        expect(field.clone()).to_have_value(&mask).await?;
        page.get_by_role(Button,named("Reveal")).click(None).await?;
        expect(field.clone()).to_have_value(&secret).await?;
        page.get_by_role(Button,named("Hide")).click(None).await?;
        expect(field.clone()).to_have_attribute("type","text").await?;
        assert!(ui.requests.lock().unwrap().is_empty());
        assert_eq!(page.locator(".key-row code").count().await?,0);
        page.get_by_label("Name for Checkout service",true).fill("Checkout worker",None).await?;
        page.get_by_role(Button,named("Save name for Checkout service")).click(None).await?;
        expect(page.get_by_label("Name for Checkout worker",true)).to_have_value("Checkout worker").await?;
        expect(page.locator(".key-row").get_by_role(Status,None)).to_have_text("Saved").await?;
        server.stop().await?; server.start().await?;
        page.reload(None).await?;
        sign_in(page,"admin",PASSWORD).await?;
        expect(page.get_by_label("Name for Checkout worker",true)).to_have_value("Checkout worker").await?;
        expect(field.clone()).to_have_attribute("type","text").await?;
        expect(field.clone()).to_have_value(&mask).await?;
        copy.focus().await?; page.keyboard().press("Enter",None).await?;
        expect(page.get_by_role(Status,None)).to_have_text("Copied").await?;
        assert_eq!(js(page,"async () => window.partlyCopied ?? await navigator.clipboard.readText()").await?,secret);
        page.set_viewport_size(Viewport{width:390,height:844}).await?;
        assert_eq!(js(page,"document.documentElement.scrollWidth <= innerWidth").await?,true);
        page.evaluate_expression("() => { navigator.clipboard.writeText = async () => { throw new DOMException('Clipboard blocked', 'NotAllowedError'); }; }").await?;
        copy.click(None).await?;
        expect(page.get_by_role(Alert,None)).to_contain_text("Could not copy").await?;
        expect(field).to_have_attribute("type","text").await?;
        choose(page,page.get_by_label("Environment",true),"production").await?;
        expect_page(page).to_have_url(&format!("{}/environments/production/apps",server.url)).await?;
        expect(page.get_by_text("No applications in this environment.",true)).to_be_visible().await?;
        page.get_by_role(Tab,named("Feature flags")).click(None).await?;
        expect(page.get_by_text("No flags in this environment",true)).to_be_visible().await?;
        choose(page,page.get_by_label("Environment",true),"development").await?;
        expect(page.get_by_label("Percentage for new_checkout",true)).to_have_value("50").await?;
        page.get_by_role(Button,named("Delete new_checkout")).click(None).await?;
        confirm_flag(page,"new_checkout").await?;
        expect(page.get_by_text("No flags in this environment",true)).to_be_visible().await?;
        page.get_by_role(Tab,named("Apps & Keys")).click(None).await?;
        page.get_by_role(Button,named("Revoke")).click(None).await?;
        page.get_by_role(Dialog,named("Revoke API Key?")).get_by_role(Button,named("Revoke key")).click(None).await?;
        expect(page.get_by_text("Revoked keys (1)",true)).to_be_visible().await?;
        page.get_by_role(Button,named("Sign out")).click(None).await?;
        expect(page.get_by_role(Button,named("Sign in"))).to_be_visible().await?;
        expect(page.get_by_role(Tablist,None)).to_be_hidden().await?;
        assert_eq!(page.locator(".key-controls input").count().await?,0);
        Ok(())
    }).await
}

#[tokio::test]
async fn app_selection_confirmation_revoked_history_and_audits() -> Result {
    let (server, admin) = fixture().await?;
    run("apps-and-audits",async |ui| {
        let page = &ui.page;
        page.add_init_script("Object.defineProperty(navigator, 'clipboard', {value: {writeText: async value => { window.partlyCopied = value; }}})").await?;
        login(page,&server,"flags","development").await?;
        create_flag(page,"shared","enabled",None).await?;
        create_flag(page,"unused","disabled",None).await?;
        choose(page,page.get_by_label("Rollout mode for unused",true),"enabled").await?;
        page.get_by_role(Button,named("Save unused")).click(None).await?;
        let modal = page.locator("#confirm-flag-dialog");
        expect(modal.get_by_text("No applications use this flag.",true)).to_be_visible().await?;
        expect(modal.locator("#confirm-flag-submit")).to_be_disabled().await?;
        modal.locator("#confirm-flag-input").fill("production/unused",None).await?;
        expect(modal.locator("#confirm-flag-submit")).to_be_disabled().await?;
        modal.get_by_role(Button,named("Keep editing")).click(None).await?;
        expect(page.get_by_role(Button,named("Save unused"))).to_be_enabled().await?;
        page.get_by_role(Button,named("Save unused")).click(None).await?;
        confirm_flag(page,"unused").await?;
        page.get_by_role(Tab,named("Apps & Keys")).click(None).await?;
        for name in ["Billing","Storefront"] {
            page.get_by_role(Button,named("Create app")).click(None).await?;
            let create = page.get_by_role(Dialog,named("Create app"));
            create.get_by_label("Application name",true).fill(name,None).await?;
            expect(create.get_by_role(Checkbox,named("shared"))).not().to_be_checked().await?;
            create.get_by_role(Checkbox,named("shared")).check(None).await?;
            create.get_by_role(Button,named("Create app")).click(None).await?;
            expect(create).to_be_hidden().await?;
        }
        let billing = page.locator(".app-row").filter(FilterOptions::default().has(page.get_by_role(Heading,named("Billing"))));
        let id = billing.get_attribute("data-app-id").await?.unwrap();
        assert!(id.starts_with("app_") && id.len()==24 && id[4..].chars().all(|c|c.is_ascii_hexdigit()));
        billing.get_by_role(Button,named("Copy ID")).click(None).await?;
        assert_eq!(js(page,"window.partlyCopied").await?,id);
        billing.locator(".app-settings summary").click(None).await?;
        billing.get_by_role(Checkbox,named("unused")).check(None).await?;
        page.get_by_role(Tab,named("Docs")).click(None).await?;
        expect(page.get_by_role(Dialog,named("Discard your changes?"))).to_be_visible().await?;
        page.get_by_role(Button,named("Keep editing")).click(None).await?;
        billing.get_by_role(Button,named("Save app")).click(None).await?;
        expect(billing.get_by_role(Button,named("Save app"))).to_be_disabled().await?;
        page.get_by_role(Tab,named("Feature flags")).click(None).await?;
        choose(page,page.get_by_label("Rollout mode for shared",true),"disabled").await?;
        page.get_by_role(Button,named("Save shared")).click(None).await?;
        texts(modal.locator("li"),&["Billing","Storefront"]).await?;
        expect(modal.locator("#confirm-flag-label code")).to_have_text("development/shared").await?;
        expect(modal.get_by_label("Type development/shared to confirm",true)).to_be_visible().await?;
        modal.locator("#confirm-flag-input").fill("development/Shared",None).await?;
        expect(modal.locator("#confirm-flag-submit")).to_be_disabled().await?;
        confirm_flag(page,"shared").await?;
        page.get_by_role(Tab,named("Audits")).click(None).await?;
        let latest = page.locator(".audit-row").first();
        expect(latest.locator(".type-pill")).to_have_text("Flag").await?;
        latest.locator("summary").click(None).await?;
        expect(latest.locator(".audit-detail")).to_contain_text("Billing, Storefront").await?;
        texts(latest.locator(".audit-value"),&["Enabled for everyone","Disabled for everyone"]).await?;
        expect(latest.locator(".audit-arrow")).to_have_text("→").await?;
        choose(page,page.get_by_label("Filter by action",true),"updated").await?;
        choose(page,page.get_by_label("Filter by application",true),&id).await?;
        expect(page.locator(".audit-row")).to_have_count(2).await?;
        page.get_by_role(Tab,named("Apps & Keys")).click(None).await?;
        let field = billing.get_by_label("API Key for Default",true);
        let mask = field.input_value(None).await?;
        assert!(mask.contains('•'));
        billing.get_by_role(Button,named("Reveal")).click(None).await?;
        let secret = field.input_value(None).await?;
        assert!(secret.starts_with(&mask[..7]) && secret.ends_with(&mask[mask.len()-4..]));
        billing.get_by_role(Button,named("Hide")).click(None).await?;
        expect(field).to_have_value(&mask).await?;
        billing.get_by_role(Button,named("Revoke")).click(None).await?;
        page.get_by_role(Dialog,named("Revoke API Key?")).get_by_role(Button,named("Revoke key")).click(None).await?;
        expect(billing.get_by_text("Revoked keys (1)",true)).to_be_visible().await?;
        billing.locator(".revoked-keys summary").click(None).await?;
        expect(billing.locator(".revoked-key code")).to_have_text(&mask).await?;
        assert_eq!(billing.locator(".revoked-key button, .revoked-key input").count().await?,0);
        let apps = admin.get("/api/environments/development/apps").await?;
        let retained = &apps["applications"].as_array().unwrap().iter().find(|app|app["id"]==id).unwrap()["keys"][0];
        assert!(retained["secret"].is_null() && !retained["revoked_at"].is_null());
        page.set_viewport_size(Viewport{width:390,height:844}).await?;
        assert_eq!(js(page,"document.documentElement.scrollWidth <= innerWidth").await?,true);
        Ok(())
    }).await
}

#[tokio::test]
async fn key_revocation_modal_cancel_focus_failure_and_retry() -> Result {
    let (server, _) = fixture().await?;
    run("revocation-modal", async |ui| {
        let page = &ui.page;
        login(page, &server, "apps", "development").await?;
        create_key(page, "Worker <legacy>").await?;
        create_key(page, "Spare").await?;
        let row = page.locator(".key-row").filter(
            FilterOptions::default().has(page.get_by_label("Name for Worker <legacy>", true)),
        );
        let revoke = row.get_by_role(Button, named("Revoke"));
        let modal = page.get_by_role(Dialog, named("Revoke API Key?"));
        let cancel = modal.get_by_role(Button, named("Cancel"));
        let confirm = modal.get_by_role(Button, named("Revoke key"));
        let deletes = || {
            ui.requests
                .lock()
                .unwrap()
                .iter()
                .filter(|(method, _)| method == "DELETE")
                .count()
        };
        for key in ["Enter", "Escape"] {
            revoke.click(None).await?;
            expect(modal.clone()).to_be_visible().await?;
            focused(cancel.clone(), true).await?;
            expect(modal.locator("#revoke-key-context"))
                .to_have_text("Revoke Worker <legacy> for Test application in development.")
                .await?;
            expect(modal.locator("legacy")).to_have_count(0).await?;
            expect(modal.clone())
                .to_contain_text("Disconnected SDKs keep their cached policy until they reconnect.")
                .await?;
            page.keyboard().press(key, None).await?;
            expect(modal.clone()).to_be_hidden().await?;
            focused(revoke.clone(), true).await?;
            expect(row.get_by_role(Button, named("Reveal")))
                .to_be_enabled()
                .await?;
            assert_eq!(deletes(), 0);
        }
        fail_route(page, "**/api/keys/*", 503, "Temporary failure").await?;
        revoke.click(None).await?;
        page.set_viewport_size(Viewport {
            width: 390,
            height: 844,
        })
        .await?;
        assert_eq!(
            js(page, "document.documentElement.scrollWidth <= innerWidth").await?,
            true
        );
        confirm.click(None).await?;
        expect(modal.clone()).to_be_hidden().await?;
        expect(row.get_by_role(Alert, None))
            .to_have_text("Temporary failure")
            .await?;
        expect(revoke.clone()).to_be_enabled().await?;
        focused(revoke.clone(), true).await?;
        expect(page.locator(".revoked-keys")).to_be_hidden().await?;
        assert_eq!(deletes(), 1);
        page.unroute("**/api/keys/*").await?;
        revoke.click(None).await?;
        confirm.click(None).await?;
        expect(row).to_have_count(0).await?;
        expect(page.get_by_text("Revoked keys (1)", true))
            .to_be_visible()
            .await?;
        focused(page.get_by_role(Button, named("Create key")), true).await?;
        expect(page.get_by_label("Name for Spare", true))
            .to_be_visible()
            .await?;
        assert_eq!(deletes(), 2);
        assert!(ui.dialogs.lock().unwrap().is_empty());
        Ok(())
    })
    .await
}

async fn entry(page: &Page, name: &str, action: &str) -> Result<Locator> {
    let row = page
        .locator(".audit-row")
        .filter(FilterOptions::default().has(page.locator(".audit-item").get_by_text(name, true)))
        .filter(FilterOptions::default().has(page.locator("summary").get_by_text(action, true)));
    row.locator("summary").click(None).await?;
    Ok(row)
}

#[tokio::test]
async fn audit_readable_states_creation_and_responsive_layout() -> Result {
    let (server, admin) = fixture().await?;
    let base = "/api/environments/development";
    admin
        .post(
            &format!("{base}/flags"),
            json!({"key":"checkout","mode":"percentage","percentage":0}),
            201,
        )
        .await?;
    let rules = json!({"mode":"custom","action":"enable","groups":[[{"k":"age","op":"ge","v":18},{"k":"email","op":"suffix","v":"<img src=x onerror=\"alert(1)\">"},{"k":"staff","op":"is","v":false}],[{"k":"plan","op":"matches","v":""}]]});
    admin
        .put(&format!("{base}/flags/checkout"), rules.clone(), 200)
        .await?;
    let mut blocked = rules;
    blocked["key"] = "blocked".into();
    blocked["action"] = "disable".into();
    admin.post(&format!("{base}/flags"), blocked, 201).await?;
    let app = admin.app("development", "Storefront", &[]).await?;
    admin
        .put(
            &format!("{base}/apps/{}", text(&app["id"])),
            json!({"name":"Shop","flags":["checkout"]}),
            200,
        )
        .await?;
    let key = &app["keys"][0];
    admin
        .put(
            &format!("/api/keys/{}", text(&key["id"])),
            json!({"name":"Checkout server"}),
            204,
        )
        .await?;
    admin
        .delete(&format!("/api/keys/{}", text(&key["id"])), 204)
        .await?;
    admin.delete(&format!("{base}/flags/blocked"), 204).await?;
    run("audit-states", async |ui| {
        let page = &ui.page;
        page.set_viewport_size(Viewport {
            width: 1440,
            height: 1000,
        })
        .await?;
        login(page, &server, "audits", "development").await?;
        expect(page.locator(".audit-row")).to_have_count(9).await?;
        let created = entry(page, "checkout", "Created").await?;
        texts(created.locator(".audit-label"), &["Created"]).await?;
        expect(created.locator(".audit-value"))
            .to_have_text("0% of users")
            .await?;
        expect(created.locator(".audit-arrow"))
            .to_have_count(0)
            .await?;
        let updated = entry(page, "checkout", "Updated").await?;
        texts(updated.locator(".audit-label"), &["Before", "After"]).await?;
        texts(
            updated.locator(".audit-value"),
            &["0% of users", "Custom rules"],
        )
        .await?;
        texts(
            updated.locator(".audit-condition"),
            &[
                "age ≥ 18",
                "email ends with “<img src=x onerror=\"alert(1)\">”",
                "staff is false",
                "plan matches “”",
            ],
        )
        .await?;
        texts(updated.locator(".audit-join"), &["AND", "AND", "OR"]).await?;
        expect(updated.locator(".audit-state").last())
            .to_contain_text("Enable when a group matches")
            .await?;
        expect(updated.locator(".audit-state").last())
            .to_contain_text("Otherwise disabled.")
            .await?;
        let deleted = entry(page, "blocked", "Deleted").await?;
        texts(deleted.locator(".audit-label"), &["Deleted"]).await?;
        expect(deleted.locator(".audit-state"))
            .to_contain_text("Disable when a group matches")
            .await?;
        expect(deleted.locator(".audit-state"))
            .to_contain_text("Otherwise enabled.")
            .await?;
        expect(deleted.locator(".audit-arrow"))
            .to_have_count(0)
            .await?;
        let created = entry(page, "Storefront", "Created").await?;
        expect(created.locator(".audit-value"))
            .to_have_text("Storefront")
            .await?;
        expect(created.locator(".audit-state"))
            .to_contain_text("No flags selected")
            .await?;
        expect(created.locator(".audit-detail"))
            .not()
            .to_contain_text("Shop")
            .await?;
        let app_updated = entry(page, "Shop", "Updated").await?;
        texts(app_updated.locator(".audit-value"), &["Storefront", "Shop"]).await?;
        expect(app_updated.locator(".audit-flags"))
            .to_have_text("checkout")
            .await?;
        let created = entry(page, "Default", "Created").await?;
        expect(created.locator(".audit-value"))
            .to_have_text("Active")
            .await?;
        expect(created.locator(".audit-state code"))
            .to_have_text(text(&key["masked_secret"]))
            .await?;
        texts(
            entry(page, "Checkout server", "Renamed")
                .await?
                .locator(".audit-value"),
            &["Default", "Checkout server"],
        )
        .await?;
        texts(
            entry(page, "Checkout server", "Revoked")
                .await?
                .locator(".audit-value"),
            &["Active", "Revoked"],
        )
        .await?;
        assert_eq!(
            page.locator(".audit-detail pre, .audit-detail img")
                .count()
                .await?,
            0
        );
        let audits = page.locator("#audits").inner_text().await?;
        assert!(!audits.contains("null") && !audits.contains(text(&key["secret"])));
        let states = updated.locator(".audit-state");
        let left = states.nth(0).bounding_box().await?.unwrap();
        let right = states.nth(1).bounding_box().await?.unwrap();
        assert!((left.width - right.width).abs() < 2.0 && right.x > left.x + left.width);
        let summary = updated.locator("summary").bounding_box().await?.unwrap();
        let timestamp = updated.locator("time").bounding_box().await?.unwrap();
        assert!((summary.x + summary.width - timestamp.x - timestamp.width).abs() <= 9.0);
        for width in [768, 390] {
            page.set_viewport_size(Viewport { width, height: 844 })
                .await?;
            assert_eq!(
                js(page, "document.documentElement.scrollWidth <= innerWidth").await?,
                true
            );
        }
        let left = states.nth(0).bounding_box().await?.unwrap();
        let right = states.nth(1).bounding_box().await?.unwrap();
        assert!((left.x - right.x).abs() < 2.0 && right.y > left.y + left.height);
        Ok(())
    })
    .await
}

#[tokio::test]
async fn users_dashboard_and_read_only_access() -> Result {
    let (server, admin) = fixture().await?;
    admin
        .flag("development", "new_checkout", json!({"mode":"enabled"}))
        .await?;
    let key = admin.key("development").await?;
    run("users-and-reader", async |ui| {
        let page = &ui.page;
        login(page, &server, "users", "development").await?;
        ui.requests.lock().unwrap().clear();
        page.get_by_role(Button, named("Create user"))
            .click(None)
            .await?;
        let modal = page.get_by_role(Dialog, named("Create user"));
        modal
            .get_by_label("Username", true)
            .fill("reader", None)
            .await?;
        modal
            .get_by_label("Password", true)
            .fill(PASSWORD, None)
            .await?;
        choose(
            page,
            modal.get_by_label("development access", true),
            "viewer",
        )
        .await?;
        modal
            .get_by_role(Button, named("Create user"))
            .click(None)
            .await?;
        expect(modal).to_be_hidden().await?;
        expect(page.get_by_role(Button, named("Edit reader")))
            .to_be_visible()
            .await?;
        focused(page.get_by_role(Button, named("Edit reader")), true).await?;
        assert!(!ui
            .requests
            .lock()
            .unwrap()
            .contains(&("GET".into(), format!("{}/api/users", server.url))));
        page.get_by_role(Button, named("Edit reader"))
            .click(None)
            .await?;
        let modal = page.get_by_role(Dialog, named("Edit reader"));
        expect(modal.get_by_label("Username", true))
            .to_have_attribute("readonly", "")
            .await?;
        expect(modal.get_by_role(Button, named("Save changes")))
            .to_be_disabled()
            .await?;
        choose(
            page,
            modal.get_by_label("production access", true),
            "editor",
        )
        .await?;
        modal
            .get_by_role(Button, named("Cancel"))
            .click(None)
            .await?;
        expect(page.get_by_role(Dialog, named("Discard your changes?")))
            .to_be_visible()
            .await?;
        page.get_by_role(Button, named("Discard changes"))
            .click(None)
            .await?;
        expect(modal).to_be_hidden().await?;
        page.get_by_role(Button, named("Edit admin"))
            .click(None)
            .await?;
        expect(page.get_by_label("Instance administrator", true))
            .to_be_disabled()
            .await?;
        expect(page.get_by_label("Account enabled", true))
            .to_be_disabled()
            .await?;
        page.get_by_role(Dialog, named("Edit admin"))
            .get_by_role(Button, named("Cancel"))
            .click(None)
            .await?;
        for width in [1280, 390] {
            page.set_viewport_size(Viewport { width, height: 900 })
                .await?;
            assert_eq!(
                js(page, "document.documentElement.scrollWidth <= innerWidth").await?,
                true
            );
        }
        page.get_by_role(Button, named("Sign out"))
            .click(None)
            .await?;
        expect(page.get_by_role(Button, named("Sign in")))
            .to_be_visible()
            .await?;
        page.goto(&server.url, None).await?;
        sign_in(page, "reader", PASSWORD).await?;
        expect_page(page)
            .to_have_url(&format!("{}/environments/development/flags", server.url))
            .await?;
        expect(page.get_by_role(Tab, named("Users")))
            .to_have_count(0)
            .await?;
        expect(page.get_by_role(Button, named("Create flag")))
            .to_be_hidden()
            .await?;
        expect(page.get_by_label("Rollout mode for new_checkout", true))
            .to_be_disabled()
            .await?;
        page.get_by_label("Environment", true).click(None).await?;
        texts(
            page.locator("#dropdown-options")
                .get_by_role(ListOption, None),
            &["Development"],
        )
        .await?;
        page.keyboard().press("Escape", None).await?;
        page.get_by_role(Tab, named("Apps & Keys"))
            .click(None)
            .await?;
        expect(page.locator(".key-row")).to_be_visible().await?;
        expect(page.get_by_role(Button, named("Reveal")))
            .to_have_count(0)
            .await?;
        expect(page.get_by_role(Button, named("Create app")))
            .to_be_hidden()
            .await?;
        assert!(!page.content().await?.contains(text(&key["secret"])));
        page.get_by_role(Tab, named("Feature flags"))
            .focus()
            .await?;
        page.keyboard().press("End", None).await?;
        focused(page.get_by_role(Tab, named("Docs")), true).await?;
        page.goto(
            &format!("{}/environments/production/apps", server.url),
            None,
        )
        .await?;
        expect(page.get_by_role(Alert, None))
            .to_contain_text("You do not have access to this environment")
            .await?;
        expect(page.locator("#workspace")).to_be_hidden().await?;
        page.locator(".brand").click(None).await?;
        expect_page(page)
            .to_have_url(&format!("{}/environments/development/flags", server.url))
            .await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn user_without_environment_gets_clear_access_message() -> Result {
    let (server, admin) = fixture().await?;
    admin.user("unassigned", json!({}), false).await?;
    run("unassigned-user", async |ui| {
        let page = &ui.page;
        page.goto(&server.url, None).await?;
        sign_in(page, "unassigned", PASSWORD).await?;
        expect(page.get_by_role(Alert, None))
            .to_contain_text("Ask an administrator to assign access")
            .await?;
        expect(page.locator("#workspace")).to_be_hidden().await?;
        expect(page.get_by_role(Button, named("Sign out")))
            .to_be_visible()
            .await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn https_dashboard_setup_login_flags_and_keys() -> Result {
    let mut server = Server::new()?;
    let tls = Tls::new("127.0.0.1", false)?;
    tls.configure(&mut server);
    server.start().await?;
    run("https-dashboard", async |ui| {
        let page = &ui.page;
        let writes = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
        let captured = writes.clone();
        page.on_request(move |request| {
            if ["POST", "PUT", "DELETE"].contains(&request.method()) {
                captured.lock().unwrap().push(request);
            }
            async { Ok(()) }
        })
        .await?;
        page.goto(&server.url, None).await?;
        expect(page.get_by_role(Button, named("Create admin account")))
            .to_be_visible()
            .await?;
        assert!(!server.db.exists());
        page.locator("#auth")
            .get_by_label("Username", true)
            .fill("admin", None)
            .await?;
        page.locator("#auth")
            .get_by_label("Password", true)
            .fill(PASSWORD, None)
            .await?;
        page.get_by_role(Button, named("Create admin account"))
            .click(None)
            .await?;
        expect_page(page)
            .to_have_url(&format!("{}/environments/development/flags", server.url))
            .await?;
        assert_eq!(
            js(
                page,
                "performance.getEntriesByType('navigation')[0].nextHopProtocol"
            )
            .await?,
            "h2"
        );
        let cookies = page.context().unwrap().cookies(None).await?;
        let session = cookies
            .iter()
            .find(|cookie| cookie.name == "partly_session")
            .unwrap();
        assert!(session.secure && session.http_only);
        assert_eq!(session.same_site.as_deref(), Some("Strict"));
        create_flag(page, "new_checkout", "enabled", None).await?;
        page.get_by_role(Tab, named("Apps & Keys"))
            .click(None)
            .await?;
        create_key(page, "HTTPS application").await?;
        expect(page.locator(".key-controls input"))
            .to_have_value_regex("^pk_[A-Za-z0-9]{4}•+[A-Za-z0-9]{4}$")
            .await?;
        page.get_by_role(Button, named("Sign out"))
            .click(None)
            .await?;
        expect(page.get_by_role(Button, named("Sign in")))
            .to_be_visible()
            .await?;
        login(page, &server, "apps", "development").await?;
        expect(page.get_by_label("Name for HTTPS application", true))
            .to_be_visible()
            .await?;
        let writes = writes.lock().unwrap().clone();
        assert!(!writes.is_empty());
        for request in writes {
            assert!(request.url().starts_with(&format!("{}/api/", server.url)));
            assert_eq!(
                request.header_value("origin").await?.as_deref(),
                Some(server.url.as_str())
            );
        }
        Ok(())
    })
    .await
}
