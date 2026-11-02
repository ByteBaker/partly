// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;

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
        expect(page.locator(".flag-row[data-key=unused]")).to_have_attribute("data-dirty", "false").await?;
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
        expect(billing.locator(".app-settings form")).to_have_attribute("data-dirty", "false").await?;
        expect(billing.get_by_role(Button,named("Save app"))).to_be_disabled().await?;
        page.get_by_role(Tab,named("Feature flags")).click(None).await?;
        choose(page,page.get_by_label("Rollout mode for shared",true),"disabled").await?;
        let mut held = hold_route(page, "**/api/environments/development/flags/shared").await?;
        page.get_by_role(Button,named("Save shared")).click(None).await?;
        texts(modal.locator("li"),&["Billing","Storefront"]).await?;
        expect(modal.locator("#confirm-flag-label code")).to_have_text("development/shared").await?;
        expect(modal.get_by_label("Type development/shared to confirm",true)).to_be_visible().await?;
        modal.locator("#confirm-flag-input").fill("development/Shared",None).await?;
        expect(modal.locator("#confirm-flag-submit")).to_be_disabled().await?;
        confirm_flag(page,"shared").await?;
        // Closing confirmation starts the write; it does not mean the write has completed.
        expect(page.locator(".flag-row[data-key=shared]")).to_have_attribute("data-dirty", "true").await?;
        pending(&mut held).await?.continue_(None).await?;
        expect(page.locator(".flag-row[data-key=shared]")).to_have_attribute("data-dirty", "false").await?;
        page.unroute("**/api/environments/development/flags/shared").await?;
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
                .to_contain_text("policy refresh or usage report confirms revocation")
                .await?;
            expect(modal.clone())
                .to_contain_text(
                    "Offline SDKs keep their cached policy until they receive that response.",
                )
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
