// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;

#[tokio::test]
async fn failed_save_preserves_draft_and_key_rename_warns() -> Result {
    let (server, _) = fixture().await?;
    run("failed-save", async |ui| {
        let page = &ui.page;
        login(page, &server, "flags", "development").await?;
        create_flag(page, "checkout", "percentage", Some(50)).await?;
        let field = page.get_by_label("Percentage for checkout", true);
        field.fill("75", None).await?;
        fail_route(
            page,
            "**/api/environments/development/flags/checkout",
            500,
            "Temporary failure",
        )
        .await?;
        page.get_by_role(Button, named("Save checkout"))
            .click(None)
            .await?;
        confirm_flag(page, "checkout").await?;
        expect(page.get_by_role(Alert, None))
            .to_have_text("Temporary failure")
            .await?;
        expect(field).to_have_value("75").await?;
        expect(page.get_by_role(Button, named("Save checkout")))
            .to_be_enabled()
            .await?;
        page.get_by_role(Tab, named("Apps & Keys"))
            .click(None)
            .await?;
        page.get_by_role(Button, named("Discard changes"))
            .click(None)
            .await?;
        create_key(page, "Worker").await?;
        let name = page.get_by_label("Name for Worker", true);
        name.fill("Renamed worker", None).await?;
        choose(page, page.get_by_label("Environment", true), "production").await?;
        expect(page.get_by_role(Dialog, named("Discard your changes?")))
            .to_be_visible()
            .await?;
        page.get_by_role(Button, named("Keep editing"))
            .click(None)
            .await?;
        expect(name.clone()).to_have_value("Renamed worker").await?;
        name.fill("Worker", None).await?;
        expect(page.get_by_role(Button, named("Save name for Worker")))
            .to_be_disabled()
            .await?;
        page.get_by_role(Tab, named("Docs")).click(None).await?;
        expect_page(page)
            .to_have_url(&format!("{}/environments/development/docs", server.url))
            .await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn pending_save_locks_only_submitted_form() -> Result {
    let (server, _) = fixture().await?;
    run("pending-save", async |ui| {
        let page = &ui.page;
        login(page, &server, "flags", "development").await?;
        create_flag(page, "checkout", "percentage", Some(50)).await?;
        create_flag(page, "search", "percentage", Some(25)).await?;
        let mut held = hold_route(page, "**/api/environments/development/flags/checkout").await?;
        let field = page.get_by_label("Percentage for checkout", true);
        field.fill("75", None).await?;
        page.get_by_role(Button, named("Save checkout"))
            .click(None)
            .await?;
        confirm_flag(page, "checkout").await?;
        expect(field.clone()).to_be_disabled().await?;
        expect(page.get_by_label("Rollout mode for checkout", true))
            .to_be_disabled()
            .await?;
        let other = page.get_by_label("Percentage for search", true);
        expect(other.clone()).to_be_enabled().await?;
        other.fill("40", None).await?;
        pending(&mut held).await?.continue_(None).await?;
        expect(field.clone()).to_be_enabled().await?;
        expect(field).to_have_value("75").await?;
        expect(page.get_by_role(Button, named("Save checkout")))
            .to_be_disabled()
            .await?;
        expect(other.clone()).to_have_value("40").await?;
        focused(other, true).await?;
        expect(page.get_by_role(Button, named("Save search")))
            .to_be_enabled()
            .await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn key_requests_are_serialized_and_session_failure_keeps_draft() -> Result {
    let (server, _) = fixture().await?;
    run("key-session-failure", async |ui| {
        let page = &ui.page;
        login(page, &server, "apps", "development").await?;
        create_key(page, "Worker").await?;
        let name = page.get_by_label("Name for Worker", true);
        name.fill("Worker v2", None).await?;
        let mut held = hold_route(page, "**/api/keys/*").await?;
        page.get_by_role(Button, named("Save name for Worker"))
            .click(None)
            .await?;
        expect(name.clone()).to_be_disabled().await?;
        expect(page.get_by_role(Button, named("Revoke")))
            .to_be_disabled()
            .await?;
        expect(page.get_by_role(Button, named("Reveal")))
            .to_be_disabled()
            .await?;
        pending(&mut held)
            .await?
            .fulfill(
                FulfillOptions::builder()
                    .status(401)
                    .json(&json!({"error":"Unauthorized"}))?
                    .build(),
            )
            .await?;
        expect(page.locator(".key-row").get_by_role(Alert, None))
            .to_contain_text("Your session has ended")
            .await?;
        expect(name.clone()).to_have_value("Worker v2").await?;
        expect(name).to_be_enabled().await?;
        expect(page.get_by_role(Button, named("Revoke")))
            .to_be_enabled()
            .await?;
        page.get_by_role(Tab, named("Feature flags"))
            .click(None)
            .await?;
        expect(page.get_by_role(Dialog, named("Discard your changes?")))
            .to_be_visible()
            .await?;
        page.get_by_role(Button, named("Keep editing"))
            .click(None)
            .await?;
        page.unroute("**/api/keys/*").await?;
        page.get_by_role(Button, named("Save name for Worker"))
            .click(None)
            .await?;
        expect(page.get_by_label("Name for Worker v2", true))
            .to_have_value("Worker v2")
            .await?;
        focused(page.get_by_label("Name for Worker v2", true), true).await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn pending_creation_cannot_close_or_replace_draft() -> Result {
    let (server, _) = fixture().await?;
    run("pending-creation", async |ui| {
        let page = &ui.page;
        login(page, &server, "flags", "development").await?;
        page.get_by_role(Button, named("Create flag"))
            .click(None)
            .await?;
        let modal = page.get_by_role(Dialog, named("Create a flag"));
        modal
            .get_by_label("Flag key", true)
            .fill("pending_flag", None)
            .await?;
        let mut held = hold_route(page, "**/api/environments/development/flags").await?;
        modal
            .get_by_role(Button, named("Create flag"))
            .click(None)
            .await?;
        expect(modal.get_by_label("Flag key", true))
            .to_be_disabled()
            .await?;
        expect(modal.get_by_role(Button, named("Close flag dialog")))
            .to_be_disabled()
            .await?;
        expect(modal.get_by_role(Button, named("Cancel")))
            .to_be_disabled()
            .await?;
        page.keyboard().press("Escape", None).await?;
        expect(modal.clone()).to_be_visible().await?;
        expect(page.get_by_role(Dialog, named("Discard your changes?")))
            .to_be_hidden()
            .await?;
        pending(&mut held).await?.continue_(None).await?;
        expect(modal.clone()).to_be_hidden().await?;
        expect(page.locator(".flag-row[data-key=pending_flag]"))
            .to_be_visible()
            .await?;
        page.get_by_role(Button, named("Create flag"))
            .click(None)
            .await?;
        expect(modal.get_by_role(Button, named("Close flag dialog")))
            .to_be_enabled()
            .await?;
        expect(modal.get_by_label("Flag key", true))
            .to_have_value("")
            .await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn key_modal_dirty_and_pending_creation_safeguards() -> Result {
    let (server, _) = fixture().await?;
    run("pending-key", async |ui| {
        let page = &ui.page;
        login(page, &server, "apps", "development").await?;
        open_key(page).await?;
        let modal = page.get_by_role(Dialog, named("Generate API Key"));
        focused(modal.get_by_label("Key name", true), true).await?;
        expect(modal.get_by_text(
            "Generate API Key for Test application in development.",
            true,
        ))
        .to_be_visible()
        .await?;
        modal
            .get_by_label("Key name", true)
            .fill("Checkout service", None)
            .await?;
        page.keyboard().press("Escape", None).await?;
        expect(page.get_by_role(Dialog, named("Discard your changes?")))
            .to_be_visible()
            .await?;
        page.get_by_role(Button, named("Keep editing"))
            .click(None)
            .await?;
        expect(modal.get_by_label("Key name", true))
            .to_have_value("Checkout service")
            .await?;
        let mut held = hold_route(page, "**/api/environments/development/apps/*/keys").await?;
        modal
            .get_by_role(Button, named("Generate key"))
            .click(None)
            .await?;
        expect(modal.get_by_label("Key name", true))
            .to_be_disabled()
            .await?;
        expect(modal.get_by_role(Button, named("Close key dialog")))
            .to_be_disabled()
            .await?;
        page.keyboard().press("Escape", None).await?;
        expect(modal.clone()).to_be_visible().await?;
        pending(&mut held).await?.continue_(None).await?;
        expect(modal.clone()).to_be_hidden().await?;
        expect(page.get_by_label("API Key for Checkout service", true))
            .to_have_attribute("type", "text")
            .await?;
        open_key(page).await?;
        expect(modal.get_by_label("Key name", true))
            .to_have_value("")
            .await?;
        modal
            .get_by_role(Button, named("Cancel"))
            .click(None)
            .await?;
        expect(modal).to_be_hidden().await?;
        expect(page.get_by_role(Dialog, named("Discard your changes?")))
            .to_be_hidden()
            .await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn success_feedback_never_moves_existing_content() -> Result {
    let (server, _) = fixture().await?;
    run("stable-feedback",async |ui| {
        let page = &ui.page;
        page.add_init_script("Object.defineProperty(navigator, 'clipboard', {value: {writeText: async text => { window.partlyCopied = text; }}})").await?;
        login(page,&server,"flags","development").await?;
        create_flag(page,"checkout","percentage",Some(50)).await?;
        page.get_by_role(Tab,named("Apps & Keys")).click(None).await?;
        create_key(page,"Worker").await?; create_key(page,"Other worker").await?;
        page.clock()?.install(None).await?;
        for width in [1280,390,320] {
            page.set_viewport_size(Viewport{width,height:900}).await?;
            expect(page.locator(".key-row")).to_have_count(2).await?;
            let selector = ".workspace-heading, .key-row, .key-row input, .key-row button, #apps-screen > .hint";
            let before = geometry(page,selector).await?;
            let row = page.locator(".key-row").first();
            row.get_by_role(Button,containing("Copy API Key ")).click(None).await?;
            expect(row.get_by_role(Status,None)).to_have_text("Copied").await?;
            assert_eq!(geometry(page,selector).await?,before);
            page.clock()?.fast_forward(3000).await?;
            expect(page.get_by_role(Status,None)).to_be_hidden().await?;
            assert_eq!(geometry(page,selector).await?,before);
            row.locator("input[name=name]").fill(&format!("Worker {width}"), None).await?;
            row.get_by_role(Button,containing("Save name for ")).click(None).await?;
            expect(row.get_by_role(Status,None)).to_have_text("Saved").await?;
            assert_eq!(geometry(page,selector).await?,before);
            page.get_by_role(Tab,named("Docs")).click(None).await?;
            expect(page.get_by_role(Button,named("Copy code"))).to_be_enabled().await?;
            let selector = ".workspace-heading, .code-panel, .code-heading, .code-heading > span, .code-heading label, #copy-example, .code-example";
            let before = geometry(page,selector).await?;
            page.get_by_role(Button,named("Copy code")).click(None).await?;
            expect(page.get_by_role(Status,None)).to_have_text("Copied").await?;
            assert_eq!(geometry(page,selector).await?,before);
            assert_eq!(js(page,"document.documentElement.scrollWidth <= innerWidth").await?,true);
            page.clock()?.fast_forward(3000).await?;
            expect(page.get_by_role(Status,None)).to_be_hidden().await?;
            assert_eq!(geometry(page,selector).await?,before);
            page.get_by_role(Tab,named("Feature flags")).click(None).await?;
            page.get_by_label("Percentage for checkout",true).fill(&(width%100).to_string(), None).await?;
            let selector = ".workspace-heading, .flag-row, .flag-heading, .flag-row .dropdown, .flag-row input, .flag-row button";
            let before = geometry(page,selector).await?;
            page.get_by_role(Button,named("Save checkout")).click(None).await?;
            confirm_flag(page,"checkout").await?;
            expect(page.get_by_role(Status,None)).to_have_text("Saved").await?;
            assert_eq!(geometry(page,selector).await?,before);
            page.clock()?.fast_forward(3000).await?;
            expect(page.get_by_role(Status,None)).to_be_hidden().await?;
            assert_eq!(geometry(page,selector).await?,before);
            page.get_by_role(Tab,named("Apps & Keys")).click(None).await?;
        }
        Ok(())
    }).await
}
