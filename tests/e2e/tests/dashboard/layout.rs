// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;

#[tokio::test]
async fn headers_select_clearance_and_sticky_navigation() -> Result {
    let (server, _) = fixture().await?;
    run("sticky-navigation",async |ui| {
        let page = &ui.page;
        login(page,&server,"flags","production").await?;
        for width in [1280,390,320] {
            page.set_viewport_size(Viewport{width,height:900}).await?;
            let flag_button = geometry(page,"#create-flag").await?;
            page.get_by_role(Tab,named("Apps & Keys")).click(None).await?;
            expect(page.locator("#create-app")).to_be_visible().await?;
            assert_eq!(geometry(page,"#create-app").await?,flag_button);
            expect(page.get_by_text("API Keys can read feature flags and rules. Use the UI to make changes.",true)).to_be_visible().await?;
            page.get_by_role(Tab,named("Docs")).click(None).await?;
            expect(page.locator(".docs-guide, footer")).to_have_count(0).await?;
            expect(page.get_by_text("Default project",true)).to_have_count(0).await?;
            for dropdown in page.get_by_role(Combobox,None).all().await? {
                let geometry: serde_json::Value = dropdown.evaluate("e => { const style = getComputedStyle(e); const arrow = getComputedStyle(e.parentElement, '::after'); return { padding: parseFloat(style.paddingRight), inset: parseFloat(arrow.right), events: arrow.pointerEvents }; }",None::<()>).await?;
                assert_eq!(geometry,json!({"padding":36,"inset":14,"events":"none"}));
            }
            assert_eq!(js(page,"document.documentElement.scrollWidth <= innerWidth").await?,true);
            expect(page.locator(".brand-mark")).to_be_visible().await?;
            assert!(page.locator(".brand-mark").evaluate::<bool,()>("e => e.complete && e.naturalWidth > 0",None).await?);
            let before = geometry(page,".site-header, [role=tablist]").await?;
            page.evaluate_expression("window.scrollTo(0, document.body.scrollHeight)").await?;
            page.wait_for_function("scrollY > 0",None).await?;
            assert_eq!(geometry(page,".site-header, [role=tablist]").await?,before);
            page.get_by_role(Tab,named("Feature flags")).click(None).await?;
            expect(page.locator("#create-flag")).to_be_visible().await?;
        }
        page.get_by_role(Button,named("Create flag")).click(None).await?;
        let modal = page.get_by_role(Dialog,named("Create a flag"));
        let mode = modal.get_by_label("Rollout mode for new flag",true);
        mode.focus().await?; page.keyboard().press("c", None).await?;
        expect(mode.clone()).to_have_attribute("value","disabled").await?;
        page.keyboard().press("Enter", None).await?;
        expect(mode).to_have_attribute("value","custom").await?;
        for dropdown in modal.get_by_role(Combobox,None).all().await? {
            assert_eq!(dropdown.evaluate::<f64,()>("e => parseFloat(getComputedStyle(e).paddingRight)",None).await?,36.0);
            assert_eq!(dropdown.evaluate::<String,()>("e => getComputedStyle(e.parentElement, '::after').pointerEvents",None).await?,"none");
        }
        Ok(())
    }).await
}

#[tokio::test]
async fn long_flag_key_stays_clear_of_controls() -> Result {
    let (server, admin) = fixture().await?;
    admin
        .flag(
            "development",
            &format!("checkout_{}", "a".repeat(55)),
            json!({"mode":"enabled"}),
        )
        .await?;
    run("long-flag-key", async |ui| {
        let page = &ui.page;
        login(page, &server, "flags", "development").await?;
        let row = page.locator(".flag-row");
        for width in [1360, 900, 768, 390] {
            page.set_viewport_size(Viewport { width, height: 900 })
                .await?;
            assert_eq!(
                js(page, "document.documentElement.scrollWidth <= innerWidth").await?,
                true
            );
            let key = row.locator(".flag-name").bounding_box().await?.unwrap();
            let mode = row.locator(".mode-label").bounding_box().await?.unwrap();
            let actions = row.locator(".row-actions").bounding_box().await?.unwrap();
            assert!(key.x + key.width <= actions.x);
            assert!(key.x + key.width <= mode.x || key.y + key.height <= mode.y);
        }
        Ok(())
    })
    .await
}
