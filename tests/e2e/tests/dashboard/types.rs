// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;

#[tokio::test]
async fn long_type_names_and_values_fit_flags_types_and_audits() -> Result {
    let (server, admin) = fixture().await?;
    let enum_name = "E".repeat(64);
    let record_name = "R".repeat(64);
    let field_name = "f".repeat(64);
    let choice = "W".repeat(256);
    admin
        .post(
            "/api/environments/development/types",
            json!({"name":enum_name,"definition":{"kind":"enum","values":[choice]}}),
            201,
        )
        .await?;
    admin.post("/api/environments/development/types", json!({"name":record_name,"definition":{"kind":"record","fields":{&field_name:&enum_name}}}), 201).await?;
    admin.flag("development", "long_type", json!({"mode":"typed","result_type":record_name,"selected":{&field_name:&choice},"other":{&field_name:&choice},"rollout":{"mode":"enabled"}})).await?;
    run("long-result-types", async |ui| {
        let page = &ui.page;
        login(page, &server, "types", "development").await?;
        for (screen, selector, count) in [
            ("Types", ".type-card", 2),
            ("Feature flags", ".flag-row", 1),
            ("Audits", ".audit-row", 3),
        ] {
            page.get_by_role(Tab, named(screen)).click(None).await?;
            expect(page.locator(selector)).to_have_count(count).await?;
            if screen == "Audits" {
                for index in 0..3 {
                    page.locator(".audit-row summary")
                        .nth(index)
                        .click(None)
                        .await?;
                }
            }
            for width in [1280, 390, 320] {
                page.set_viewport_size(Viewport {
                    width,
                    height: 1000,
                })
                .await?;
                assert_eq!(
                    js(page, "document.documentElement.scrollWidth <= innerWidth").await?,
                    true,
                    "{screen} overflows at {width}"
                );
            }
        }
        Ok(())
    })
    .await
}

#[tokio::test]
async fn record_inputs_preserve_boolean_numeric_and_text_values() -> Result {
    let (server, admin) = fixture().await?;
    admin.post("/api/environments/development/types", json!({"name":"Options","definition":{"kind":"record","fields":{"active":"boolean","price":"float","message":"string"}}}), 201).await?;
    run("typed-record-inputs", async |ui| {
        let page = &ui.page;
        login(page, &server, "flags", "development").await?;
        page.get_by_role(Button, named("Create flag"))
            .click(None)
            .await?;
        let modal = page.locator("#flag-dialog");
        modal
            .get_by_label("Flag key", true)
            .fill("options", None)
            .await?;
        choose(page, modal.get_by_label("Result type", true), "Options").await?;
        let selected = modal.locator(".result-panel").first();
        let other = modal.locator(".result-panel").nth(1);
        selected
            .get_by_role(Radio, named("True"))
            .check(None)
            .await?;
        expect(other.get_by_role(Radio, named("False")))
            .to_be_checked()
            .await?;
        selected
            .get_by_label("price", true)
            .fill("1.25", None)
            .await?;
        selected
            .get_by_label("message", true)
            .fill("First\nSecond", None)
            .await?;
        modal
            .get_by_role(Button, named("Create flag"))
            .click(None)
            .await?;
        expect(modal).to_be_hidden().await?;
        let saved = admin.get("/api/environments/development/flags").await?;
        assert_eq!(
            saved["options"]["selected"],
            json!({"active":true,"price":1.25,"message":"First\nSecond"})
        );
        assert_eq!(
            saved["options"]["other"],
            json!({"active":false,"price":0,"message":""})
        );
        Ok(())
    })
    .await
}

#[tokio::test]
async fn typed_results_dashboard_creation_editing_and_audits() -> Result {
    let (server, admin) = fixture().await?;
    run("typed-results", async |ui| {
        let page = &ui.page;
        login(page, &server, "types", "development").await?;
        page.get_by_role(Button, named("Create type"))
            .click(None)
            .await?;
        let modal = page.locator("#type-dialog");
        modal
            .get_by_label("Type name", true)
            .fill("Layout", None)
            .await?;
        modal
            .get_by_label("Choices, one per line", true)
            .fill("compact\nwide", None)
            .await?;
        modal
            .get_by_role(Button, named("Create type"))
            .click(None)
            .await?;
        expect(modal.clone()).to_be_hidden().await?;
        expect(page.locator(".type-choices li"))
            .to_have_count(2)
            .await?;
        page.get_by_role(Button, named("Create type"))
            .click(None)
            .await?;
        modal
            .get_by_label("Type name", true)
            .fill("Checkout", None)
            .await?;
        choose(page, modal.get_by_label("Type kind", true), "record").await?;
        modal
            .get_by_label("Field name", true)
            .fill("layout", None)
            .await?;
        choose(page, modal.get_by_label("Field type", true), "Layout").await?;
        modal
            .get_by_role(Button, named("Add field"))
            .click(None)
            .await?;
        modal
            .get_by_label("Field name", true)
            .nth(1)
            .fill("limit", None)
            .await?;
        choose(
            page,
            modal.get_by_label("Field type", true).nth(1),
            "integer",
        )
        .await?;
        modal
            .get_by_role(Button, named("Create type"))
            .click(None)
            .await?;
        expect(modal).to_be_hidden().await?;
        expect(page.get_by_role(Button, named("Delete Layout")))
            .to_be_disabled()
            .await?;
        for width in [1280, 390, 320] {
            page.set_viewport_size(Viewport {
                width,
                height: 1000,
            })
            .await?;
            assert_eq!(
                js(page, "document.documentElement.scrollWidth <= innerWidth").await?,
                true
            );
        }
        page.set_viewport_size(Viewport {
            width: 1280,
            height: 1000,
        })
        .await?;
        page.goto(
            &format!("{}/environments/development/types", server.url),
            None,
        )
        .await?;
        expect(page.locator(".type-card")).to_have_count(2).await?;
        if std::env::var("PARTLY_BROWSER").unwrap_or_else(|_| "chromium".into()) == "chromium" {
            std::fs::create_dir_all(root().join(".cache"))?;
            std::fs::write(
                root().join(".cache/typed-types.png"),
                page.screenshot(None).await?,
            )?;
        }
        page.get_by_role(Tab, named("Feature flags"))
            .click(None)
            .await?;
        page.get_by_role(Button, named("Create flag"))
            .click(None)
            .await?;
        let modal = page.locator("#flag-dialog");
        modal
            .get_by_label("Flag key", true)
            .fill("checkout", None)
            .await?;
        choose(page, modal.get_by_label("Result type", true), "Checkout").await?;
        choose(
            page,
            modal.get_by_label("Rollout mode for new flag", true),
            "percentage",
        )
        .await?;
        let selected = modal.locator(".result-panel").first();
        choose(page, selected.get_by_label("layout", true), "wide").await?;
        selected
            .get_by_label("limit", true)
            .fill("20", None)
            .await?;
        modal
            .locator(".result-panel")
            .nth(1)
            .get_by_label("limit", true)
            .fill("5", None)
            .await?;
        modal
            .get_by_role(Button, named("Create flag"))
            .click(None)
            .await?;
        expect(modal).to_be_hidden().await?;
        let row = page.locator(".flag-row[data-key=checkout]");
        expect(row.locator(".result-panel"))
            .to_have_count(2)
            .await?;
        assert_eq!(row.get_by_label("Result type", true).count().await?, 0);
        let selected = row.locator(".result-panel").first();
        selected
            .get_by_label("limit", true)
            .fill("30", None)
            .await?;
        row.get_by_role(Button, named("Save checkout"))
            .click(None)
            .await?;
        confirm_flag(page, "checkout").await?;
        expect(page.get_by_role(Status, None))
            .to_have_text("Saved")
            .await?;
        let saved = admin.get("/api/environments/development/flags").await?;
        assert_eq!(
            saved["checkout"]["selected"],
            json!({"layout":"wide","limit":30})
        );
        for width in [1280, 390, 320] {
            page.set_viewport_size(Viewport {
                width,
                height: 1000,
            })
            .await?;
            assert_eq!(
                js(page, "document.documentElement.scrollWidth <= innerWidth").await?,
                true
            );
        }
        page.set_viewport_size(Viewport {
            width: 1280,
            height: 1000,
        })
        .await?;
        admin
            .flag(
                "development",
                "new_search",
                json!({"mode":"percentage","percentage":25}),
            )
            .await?;
        let app = admin
            .app("development", "Storefront", &["checkout", "new_search"])
            .await?;
        status(
            server
                .api(false)?
                .request(reqwest::Method::POST, "/api/usage")
                .headers(policy_headers(&app_key(&app, "development")))
                .json(&json!({"flags":["checkout", "new_search"]}))
                .send()
                .await?,
            204,
        )
        .await?;
        page.goto(
            &format!("{}/environments/development/flags", server.url),
            None,
        )
        .await?;
        expect(page.locator(".flag-row")).to_have_count(2).await?;
        if std::env::var("PARTLY_BROWSER").unwrap_or_else(|_| "chromium".into()) == "chromium" {
            std::fs::write(
                root().join(".cache/typed-flags.png"),
                page.screenshot(None).await?,
            )?;
        }
        page.get_by_role(Tab, named("Audits")).click(None).await?;
        page.locator(".audit-row")
            .filter(FilterOptions::default().has_text("Updated"))
            .first()
            .locator("summary")
            .click(None)
            .await?;
        expect(
            page.locator(".audit-row")
                .filter(FilterOptions::default().has_text("Updated"))
                .first()
                .get_by_text("30", true),
        )
        .to_be_visible()
        .await?;
        expect(
            page.locator(".audit-row")
                .filter(FilterOptions::default().has_text("Updated"))
                .first()
                .get_by_text("20", true),
        )
        .to_be_visible()
        .await?;
        assert_eq!(page.locator(".audit-row pre").count().await?, 0);
        if std::env::var("PARTLY_BROWSER").unwrap_or_else(|_| "chromium".into()) == "chromium" {
            std::fs::write(
                root().join(".cache/typed-audits.png"),
                page.screenshot(None).await?,
            )?;
        }
        page.get_by_role(Tab, named("Types")).click(None).await?;
        expect(page.get_by_role(Button, named("Delete Checkout")))
            .to_be_disabled()
            .await?;
        // A dirty type modal uses the same loss-of-edits protection as flag creation.
        page.get_by_role(Button, named("Create type"))
            .click(None)
            .await?;
        page.locator("#type-name").fill("Unsaved", None).await?;
        page.keyboard().press("Escape", None).await?;
        expect(page.locator("#discard-dialog"))
            .to_be_visible()
            .await?;
        page.get_by_role(Button, named("Keep editing"))
            .click(None)
            .await?;
        expect(page.locator("#type-dialog")).to_be_visible().await?;
        Ok(())
    })
    .await
}

#[tokio::test]
async fn typed_string_edits_preserve_existing_line_endings() -> Result {
    let (server, admin) = fixture().await?;
    let flag = json!({"mode":"typed","result_type":"string","selected":"First\r\nSecond","other":"Off\nLine","rollout":{"mode":"enabled"}});
    admin.flag("development", "message", flag).await?;
    run("typed-text", async |ui| {
        let page = &ui.page;
        login(page, &server, "flags", "development").await?;
        let row = page.locator(".flag-row");
        expect(row.get_by_label("Selected value", true))
            .to_have_value("First\nSecond")
            .await?;
        choose(
            page,
            row.get_by_label("Rollout mode for message", true),
            "disabled",
        )
        .await?;
        row.get_by_role(Button, named("Save message"))
            .click(None)
            .await?;
        confirm_flag(page, "message").await?;
        expect(page.get_by_role(Status, None))
            .to_have_text("Saved")
            .await?;
        assert_eq!(
            admin.get("/api/environments/development/flags").await?["message"]["selected"],
            "First\r\nSecond"
        );
        row.get_by_label("Selected value", true)
            .fill("New\nMessage", None)
            .await?;
        row.get_by_role(Button, named("Save message"))
            .click(None)
            .await?;
        confirm_flag(page, "message").await?;
        expect(page.get_by_role(Status, None))
            .to_have_text("Saved")
            .await?;
        assert_eq!(
            admin.get("/api/environments/development/flags").await?["message"]["selected"],
            "New\nMessage"
        );
        Ok(())
    })
    .await
}
