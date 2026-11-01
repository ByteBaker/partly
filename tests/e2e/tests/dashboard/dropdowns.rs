// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;

#[tokio::test]
async fn custom_dropdown_keyboard_commit_cancel_and_modal_escape() -> Result {
    let (server, _) = fixture().await?;
    run("dropdown-keyboard", async |ui| {
        let page = &ui.page;
        login(page, &server, "flags", "development").await?;
        page.get_by_role(Button, named("Create flag"))
            .click(None)
            .await?;
        let modal = page.get_by_role(Dialog, named("Create a flag"));
        let mode = modal.get_by_role(Combobox, named("Rollout mode for new flag"));
        let popup = page.locator("#dropdown-options");
        expect(mode.clone())
            .to_have_attribute("type", "button")
            .await?;
        expect(mode.clone())
            .to_have_attribute("value", "disabled")
            .await?;
        mode.click(None).await?;
        expect(popup.clone()).to_be_visible().await?;
        expect(mode.clone())
            .to_have_attribute("aria-expanded", "true")
            .await?;
        expect(popup.locator("[data-value='disabled']"))
            .to_have_attribute("aria-selected", "true")
            .await?;
        page.keyboard().press("Home", None).await?;
        active_option(page, &mode, "enabled").await?;
        page.keyboard().press("ArrowDown", None).await?;
        active_option(page, &mode, "percentage").await?;
        expect(mode.clone())
            .to_have_attribute("value", "disabled")
            .await?;
        expect(popup.locator("[data-value='disabled']"))
            .to_have_attribute("aria-selected", "true")
            .await?;
        page.keyboard().press("Escape", None).await?;
        expect(popup.clone()).to_be_hidden().await?;
        expect(mode.clone())
            .to_have_attribute("aria-expanded", "false")
            .await?;
        expect(mode.clone())
            .to_have_attribute("value", "disabled")
            .await?;
        expect(modal.clone()).to_be_visible().await?;
        expect(page.get_by_role(Dialog, named("Discard your changes?")))
            .to_be_hidden()
            .await?;
        focused(mode.clone(), true).await?;
        page.keyboard().press("Enter", None).await?;
        expect(popup.clone()).to_be_visible().await?;
        page.keyboard().press("End", None).await?;
        active_option(page, &mode, "disabled").await?;
        page.keyboard().press("ArrowUp", None).await?;
        active_option(page, &mode, "custom").await?;
        page.keyboard().press("Space", None).await?;
        expect(popup.clone()).to_be_hidden().await?;
        expect(mode.clone())
            .to_have_attribute("value", "custom")
            .await?;
        expect(modal.get_by_role(Combobox, named("Match action for new flag")))
            .to_be_visible()
            .await?;
        mode.focus().await?;
        page.keyboard().press("p", None).await?;
        active_option(page, &mode, "percentage").await?;
        expect(mode.clone())
            .to_have_attribute("value", "custom")
            .await?;
        page.keyboard().press("Enter", None).await?;
        expect(mode.clone())
            .to_have_attribute("value", "percentage")
            .await?;
        expect(modal.get_by_label("Percentage for new flag", true))
            .to_be_visible()
            .await?;
        mode.click(None).await?;
        page.keyboard().press("Home", None).await?;
        page.keyboard().press("Tab", None).await?;
        expect(popup.clone()).to_be_hidden().await?;
        expect(mode.clone())
            .to_have_attribute("value", "enabled")
            .await?;
        focused(mode.clone(), false).await?;
        expect(modal.clone()).to_be_visible().await?;
        for click in [true, false] {
            mode.click(None).await?;
            page.keyboard().press("End", None).await?;
            let key = modal.get_by_label("Flag key", true);
            if click {
                key.click(None).await?;
            } else {
                key.focus().await?;
            }
            expect(popup.clone()).to_be_hidden().await?;
            expect(mode.clone())
                .to_have_attribute("value", "enabled")
                .await?;
            focused(key, true).await?;
        }
        Ok(())
    })
    .await
}

#[tokio::test]
async fn environment_dropdown_only_navigates_after_commit() -> Result {
    let (server, _) = fixture().await?;
    run("environment-dropdown", async |ui| {
        let page = &ui.page;
        login(page, &server, "flags", "development").await?;
        let env = page.get_by_role(Combobox, named("Environment"));
        page.evaluate_expression("window.environmentNavigationMarker = true")
            .await?;
        env.click(None).await?;
        page.keyboard().press("End", None).await?;
        active_option(page, &env, "production").await?;
        expect(env.clone())
            .to_have_attribute("value", "development")
            .await?;
        expect_page(page)
            .to_have_url(&format!("{}/environments/development/flags", server.url))
            .await?;
        page.keyboard().press("Escape", None).await?;
        assert_eq!(js(page, "window.environmentNavigationMarker").await?, true);
        env.focus().await?;
        page.keyboard().press("p", None).await?;
        active_option(page, &env, "production").await?;
        expect(env.clone())
            .to_have_attribute("value", "development")
            .await?;
        page.keyboard().press("Tab", None).await?;
        expect_page(page)
            .to_have_url(&format!("{}/environments/production/flags", server.url))
            .await?;
        expect(env).to_have_attribute("value", "production").await?;
        assert_eq!(
            js(page, "window.environmentNavigationMarker === undefined").await?,
            true
        );
        Ok(())
    })
    .await
}

async fn fits(page: &Page, popup: &Locator) -> Result {
    let box_ = popup.bounding_box().await?.unwrap();
    let viewport = js(page, "({width: innerWidth, height: innerHeight})").await?;
    assert!(box_.x >= 0.0 && box_.y >= 0.0);
    assert!(box_.x + box_.width <= viewport["width"].as_f64().unwrap());
    assert!(box_.y + box_.height <= viewport["height"].as_f64().unwrap());
    Ok(())
}

#[tokio::test]
async fn dropdown_popup_fits_viewport_and_cancels_on_move() -> Result {
    let (server, admin) = fixture().await?;
    for index in 0..4 {
        admin
            .flag(
                "development",
                &format!("layout_{index}"),
                json!({"mode":"enabled"}),
            )
            .await?;
    }
    run("dropdown-viewport",async |ui| {
        let page = &ui.page;
        login(page,&server,"flags","development").await?;
        for width in [1280,390,320] {
            page.set_viewport_size(Viewport{width,height:640}).await?;
            let env = page.get_by_role(Combobox,named("Environment"));
            let popup = page.locator("#dropdown-options");
            let scroll = js(page,"async () => { window.scrollTo(0, 100); await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))); return scrollY; }").await?;
            assert!(scroll.as_f64().unwrap() > 0.0);
            let before = geometry(page,".site-header, #environment").await?;
            // Click the visible sticky control without Playwright's automatic scrolling.
            let rect = env.bounding_box().await?.unwrap();
            page.mouse().click(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0, None).await?;
            expect(popup.clone()).to_be_visible().await?; fits(page,&popup).await?;
            page.keyboard().press("ArrowDown", None).await?;
            active_option(page,&env,"production").await?;
            assert_eq!(js(page,"scrollY").await?,scroll);
            expect(popup.clone()).to_be_visible().await?;
            assert_eq!(geometry(page,".site-header, #environment").await?,before);
            page.keyboard().press("Escape", None).await?;
            page.get_by_role(Button,named("Create flag")).click(None).await?;
            let modal = page.get_by_role(Dialog,named("Create a flag"));
            choose(page,modal.get_by_role(Combobox,named("Rollout mode for new flag")),"custom").await?;
            let operator = modal.get_by_role(Combobox,named("Operator"));
            operator.scroll_into_view_if_needed().await?;
            let before = geometry(page,"#flag-dialog, #flag-dialog [aria-label=Operator]").await?;
            operator.click(None).await?;
            expect(popup.clone()).to_be_visible().await?;
            expect(popup.get_by_role(ListOption,None)).to_have_count(11).await?;
            assert_eq!(popup.evaluate::<String,()>("e => e.closest('dialog').id",None).await?,"flag-dialog");
            assert_eq!(geometry(page,"#flag-dialog, #flag-dialog [aria-label=Operator]").await?,before);
            fits(page,&popup).await?;
            page.keyboard().press("End", None).await?; active_option(page,&operator,"is").await?;
            let visible: bool = popup.locator("[data-value=is]").evaluate("e => { const r=e.getBoundingClientRect(), p=e.parentElement.getBoundingClientRect(); return Math.min(r.bottom,p.bottom,innerHeight)-Math.max(r.top,p.top,0) >= r.height*0.9; }",None::<()>).await?;
            assert!(visible);
            expect(operator.clone()).to_have_attribute("value","matches").await?;
            page.set_viewport_size(Viewport{width,height:630}).await?;
            expect(popup.clone()).to_be_hidden().await?;
            expect(operator.clone()).to_have_attribute("value","matches").await?;
            page.set_viewport_size(Viewport{width,height:640}).await?;
            operator.click(None).await?;
            expect(popup.clone()).to_be_visible().await?;
            modal.evaluate::<(),()>("e => { e.scrollTop = e.scrollTop ? 0 : e.scrollHeight; }",None).await?;
            expect(popup).to_be_hidden().await?;
            expect(operator).to_have_attribute("value","matches").await?;
            choose(page,modal.get_by_role(Combobox,named("Rollout mode for new flag")),"disabled").await?;
            modal.get_by_role(Button,named("Cancel")).click(None).await?;
            expect(modal).to_be_hidden().await?;
            assert_eq!(js(page,"document.documentElement.scrollWidth <= innerWidth").await?,true);
        }
        Ok(())
    }).await
}
