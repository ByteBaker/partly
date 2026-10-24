// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;
use playwright_rs::AriaRole::{Columnheader, Table};
use reqwest::Method;

#[tokio::test]
async fn usage_is_visible_per_flag_and_key_without_exposing_secrets() -> Result {
    let (server, admin) = fixture().await?;
    for key in ["checkout", "unreported"] {
        admin
            .flag("development", key, json!({"mode":"disabled"}))
            .await?;
    }
    let app = admin
        .app("development", "Checkout service", &["checkout"])
        .await?;
    let key = app_key(&app, "development");
    status(
        server
            .api(false)?
            .request(Method::POST, "/api/usage")
            .headers(policy_headers(&key))
            .json(&json!({"flags":["checkout"]}))
            .send()
            .await?,
        204,
    )
    .await?;
    run("flag-usage", async |ui| {
        let page = &ui.page;
        login(page, &server, "flags", "development").await?;
        let row = page.locator(".flag-row[data-key=checkout]");
        expect(row.locator(".flag-usage summary"))
            .to_contain_text("Last reported use")
            .await?;
        expect(page.locator(".flag-row[data-key=unreported] .flag-usage"))
            .to_have_text("No usage reported")
            .await?;
        row.locator(".flag-usage summary").focus().await?;
        page.keyboard().press("Enter", None).await?;
        expect(row.get_by_label("Usage for checkout", true))
            .to_be_visible()
            .await?;
        expect(row.locator(".flag-usage tbody"))
            .to_contain_text("Checkout service")
            .await?;
        expect(row.locator(".flag-usage tbody"))
            .to_contain_text("Default")
            .await?;
        assert!(!page.content().await?.contains(text(&key["secret"])));
        choose(
            page,
            page.get_by_label("Rollout mode for checkout", true),
            "enabled",
        )
        .await?;
        page.get_by_role(Button, named("Save checkout"))
            .click(None)
            .await?;
        confirm_flag(page, "checkout").await?;
        expect(row.locator(".flag-usage summary"))
            .to_contain_text("Last reported use")
            .await?;
        page.set_viewport_size(Viewport {
            width: 390,
            height: 844,
        })
        .await?;
        row.locator(".flag-usage summary").click(None).await?;
        assert_eq!(
            js(page, "document.documentElement.scrollWidth <= innerWidth").await?,
            true
        );
        page.get_by_role(Tab, named("Apps & Keys"))
            .click(None)
            .await?;
        expect(page.locator(".key-row"))
            .to_contain_text("Last reported use")
            .await?;
        assert_eq!(
            js(page, "document.documentElement.scrollWidth <= innerWidth").await?,
            true
        );
        Ok(())
    })
    .await
}

#[tokio::test]
async fn usage_and_audits_remain_readable_with_long_names_on_small_screens() -> Result {
    let (server, admin) = fixture().await?;
    let long_flag = "checkout_".to_owned() + &"x".repeat(55);
    let long_app = "PaymentProcessing".to_owned() + &"Service".repeat(6) + "ABCDE";
    assert_eq!(long_flag.len(), 64);
    assert_eq!(long_app.len(), 64);
    admin
        .flag("development", &long_flag, json!({"mode":"disabled"}))
        .await?;
    let app = admin.app("development", &long_app, &[&long_flag]).await?;
    let key = app_key(&app, "development");
    run("usage-responsive", async |ui| {
        let page = &ui.page;
        login(page, &server, "flags", "development").await?;
        expect(page.locator(".flag-usage"))
            .to_have_text("No usage reported")
            .await?;
        expect(page.get_by_text(
            "Usage reports arrive in batches. Missing reports do not mean a flag is unused. Reload to update.",
            true,
        ))
        .to_be_visible()
        .await?;
        status(
            server
                .api(false)?
                .request(Method::POST, "/api/usage")
                .headers(policy_headers(&key))
                .json(&json!({"flags":[long_flag]}))
                .send()
                .await?,
            204,
        )
        .await?;
        page.goto(&format!("{}/environments/development/flags", server.url), None)
            .await?;
        page.locator(".flag-usage summary").focus().await?;
        page.keyboard().press("Enter", None).await?;
        for width in [768, 390, 320] {
            page.set_viewport_size(Viewport { width, height: 844 })
                .await?;
            let table = page.get_by_role(Table, named(&format!("Usage for {long_flag}")));
            expect(table.clone()).to_be_visible().await?;
            for heading in ["Application", "API Key", "Last reported use"] {
                expect(table.get_by_role(Columnheader, named(heading)))
                    .to_have_count(1)
                    .await?;
            }
            expect(table.clone()).to_contain_text(&long_app).await?;
            assert_eq!(
                js(page, "document.documentElement.scrollWidth <= innerWidth").await?,
                true
            );
        }
        page.get_by_role(Tab, named("Apps & Keys"))
            .click(None)
            .await?;
        page.locator(".app-settings summary").click(None).await?;
        for width in [320, 390, 768] {
            page.set_viewport_size(Viewport { width, height: 844 })
                .await?;
            expect(page.get_by_role(Heading, named(&long_app)))
                .to_be_visible()
                .await?;
            expect(page.get_by_role(Checkbox, named(&long_flag)))
                .to_be_visible()
                .await?;
            assert_eq!(
                js(page, "document.documentElement.scrollWidth <= innerWidth && [...document.querySelectorAll('.flag-choices')].every(el => el.scrollWidth <= el.clientWidth)").await?,
                true
            );
        }
        page.set_viewport_size(Viewport { width: 1280, height: 1000 })
            .await?;
        let row = page.locator(".key-row").bounding_box().await?.unwrap();
        let metadata = page.locator(".key-metadata").bounding_box().await?.unwrap();
        assert!(metadata.width >= row.width - 2.0);
        expect(page.locator(".key-metadata"))
            .to_contain_text("Last reported use")
            .await?;
        page.get_by_role(Tab, named("Audits")).click(None).await?;
        for width in [390, 320] {
            page.set_viewport_size(Viewport { width, height: 844 })
                .await?;
            let filters = page.locator(".audit-filters").bounding_box().await?.unwrap();
            for label in ["Filter by action", "Filter by application"] {
                let filter = page.get_by_label(label, true).bounding_box().await?.unwrap();
                assert!(filter.width >= filters.width - 2.0);
            }
            expect(page.locator(".audit-actor").first())
                .to_have_text("by admin")
                .await?;
            assert_eq!(
                js(page, "document.documentElement.scrollWidth <= innerWidth").await?,
                true
            );
        }
        let first = page.locator(".audit-row").first();
        first.locator("summary").focus().await?;
        page.keyboard().press("Enter", None).await?;
        expect(first.locator(".audit-detail"))
            .to_be_visible()
            .await?;
        Ok(())
    })
    .await
}
