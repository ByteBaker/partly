// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;

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
        assert!(
            !ui.requests
                .lock()
                .unwrap()
                .contains(&("GET".into(), format!("{}/api/users", server.url)))
        );
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
