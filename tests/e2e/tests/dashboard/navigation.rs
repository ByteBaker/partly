// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;

#[tokio::test]
async fn login_error_allows_retry_without_layout_shift() -> Result {
    let (server, _) = fixture().await?;
    for width in [1280, 390] {
        run(&format!("login-{width}"), async |ui| {
            let page = &ui.page;
            page.set_viewport_size(Viewport { width, height: 900 })
                .await?;
            page.goto(&server.url, None).await?;
            page.locator("#username").fill("admin", None).await?;
            page.locator("#password")
                .fill("incorrect-password", None)
                .await?;
            let submit = page.get_by_role(Button, named("Sign in"));
            let before = geometry(page, "#auth, #auth button[type=submit]").await?;
            submit.click(None).await?;
            expect(page.locator("#auth").get_by_role(Alert, None))
                .to_have_text("Username or password is incorrect.")
                .await?;
            expect(page.locator("#error")).to_be_hidden().await?;
            assert_eq!(
                geometry(page, "#auth, #auth button[type=submit]").await?,
                before
            );
            expect(page.locator("#username"))
                .to_have_value("admin")
                .await?;
            expect(submit.clone()).to_be_enabled().await?;
            page.locator("#password").fill(PASSWORD, None).await?;
            submit.click(None).await?;
            expect(page.locator("#workspace")).to_be_visible().await?;
            expect(page.get_by_role(Alert, None)).to_be_hidden().await?;
            Ok(())
        })
        .await?;
    }
    Ok(())
}

#[tokio::test]
async fn unsaved_navigation_reload_and_modal_protection() -> Result {
    let (server, _) = fixture().await?;
    run("unsaved-navigation", async |ui| {
        let page = &ui.page;
        login(page, &server, "flags", "development").await?;
        create_flag(page, "checkout", "percentage", Some(50)).await?;
        let field = page.get_by_label("Percentage for checkout", true);
        field.fill("", None).await?;
        choose(page, page.get_by_label("Environment", true), "production").await?;
        let discard = page.get_by_role(Dialog, named("Discard your changes?"));
        expect(discard.clone()).to_be_visible().await?;
        page.get_by_role(Button, named("Keep editing"))
            .click(None)
            .await?;
        expect(field.clone()).to_have_value("").await?;
        expect(page.get_by_label("Environment", true))
            .to_have_attribute("value", "development")
            .await?;
        field.fill("50", None).await?;
        expect(page.get_by_role(Button, named("Save checkout")))
            .to_be_disabled()
            .await?;
        page.evaluate_expression("window.reloadMarker = true")
            .await?;
        choose(page, page.get_by_label("Environment", true), "production").await?;
        expect_page(page)
            .to_have_url(&format!("{}/environments/production/flags", server.url))
            .await?;
        assert_eq!(js(page, "window.reloadMarker === undefined").await?, true);
        choose(page, page.get_by_label("Environment", true), "development").await?;
        field.fill("75", None).await?;
        page.get_by_role(Tab, named("Docs")).click(None).await?;
        expect(discard.clone()).to_be_visible().await?;
        page.get_by_role(Button, named("Discard changes"))
            .click(None)
            .await?;
        expect_page(page)
            .to_have_url(&format!("{}/environments/development/docs", server.url))
            .await?;
        page.get_by_role(Tab, named("Feature flags"))
            .click(None)
            .await?;
        expect(field.clone()).to_have_value("50").await?;
        field.fill("75", None).await?;
        page.evaluate_expression("setTimeout(() => location.reload(), 0)")
            .await?;
        wait_for_dialog(ui).await?;
        assert_eq!(*ui.dialogs.lock().unwrap(), ["beforeunload"]);
        expect(field.clone()).to_have_value("75").await?;
        field.fill("50", None).await?;
        page.get_by_role(Button, named("Create flag"))
            .click(None)
            .await?;
        let modal = page.get_by_role(Dialog, named("Create a flag"));
        modal
            .get_by_label("Flag key", true)
            .fill("draft", None)
            .await?;
        page.keyboard().press("Escape", None).await?;
        expect(discard.clone()).to_be_visible().await?;
        page.get_by_role(Button, named("Keep editing"))
            .click(None)
            .await?;
        expect(modal.clone()).to_be_visible().await?;
        expect(modal.get_by_label("Flag key", true))
            .to_have_value("draft")
            .await?;
        modal.get_by_label("Flag key", true).fill("", None).await?;
        modal
            .get_by_role(Button, named("Cancel"))
            .click(None)
            .await?;
        expect(modal).to_be_hidden().await?;
        expect(discard.clone()).to_be_hidden().await?;
        page.get_by_role(Tab, named("Apps & Keys"))
            .click(None)
            .await?;
        open_key(page).await?;
        page.get_by_label("Key name", true)
            .fill("unsaved application", None)
            .await?;
        page.keyboard().press("Escape", None).await?;
        expect(discard).to_be_visible().await?;
        page.get_by_role(Button, named("Keep editing"))
            .click(None)
            .await?;
        expect(page.get_by_label("Key name", true))
            .to_have_value("unsaved application")
            .await?;
        Ok(())
    })
    .await
}

async fn wait_for_dialog(ui: &BrowserTest) -> Result {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while ui.dialogs.lock().unwrap().is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await?;
    Ok(())
}

#[tokio::test]
async fn browser_back_preserves_drafts_and_forward_keeps_environment() -> Result {
    let (server, _) = fixture().await?;
    run("back-forward", async |ui| {
        let page = &ui.page;
        login(page, &server, "flags", "production").await?;
        create_flag(page, "checkout", "percentage", Some(50)).await?;
        page.get_by_role(Tab, named("Apps & Keys"))
            .click(None)
            .await?;
        page.get_by_role(Tab, named("Feature flags"))
            .click(None)
            .await?;
        let field = page.get_by_label("Percentage for checkout", true);
        field.fill("75", None).await?;
        page.evaluate_expression("setTimeout(() => history.back(), 0)")
            .await?;
        wait_for_dialog(ui).await?;
        assert_eq!(*ui.dialogs.lock().unwrap(), ["beforeunload"]);
        expect_page(page)
            .to_have_url(&format!("{}/environments/production/flags", server.url))
            .await?;
        expect(field.clone()).to_have_value("75").await?;
        field.fill("50", None).await?;
        page.go_back(None).await?;
        expect_page(page)
            .to_have_url(&format!("{}/environments/production/apps", server.url))
            .await?;
        page.go_forward(None).await?;
        expect_page(page)
            .to_have_url(&format!("{}/environments/production/flags", server.url))
            .await?;
        expect(field).to_have_value("50").await?;
        expect(page.locator(".brand"))
            .to_have_attribute("href", "/environments/production/flags")
            .await?;
        Ok(())
    })
    .await
}
