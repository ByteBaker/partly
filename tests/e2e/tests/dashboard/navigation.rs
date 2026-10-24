// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;
use std::fs;

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
async fn docs_navigation_and_complete_examples() -> Result {
    let (server, _) = fixture().await?;
    run("docs-navigation",async |ui| {
        let page = &ui.page;
        page.add_init_script("Object.defineProperty(navigator, 'clipboard', {value: {writeText: async text => { window.partlyCopied = text; }}})").await?;
        login(page,&server,"flags","development").await?;
        let flags = page.get_by_role(Tab,named("Feature flags"));
        let keys = page.get_by_role(Tab,named("Apps & Keys"));
        let docs = page.get_by_role(Tab,named("Docs"));
        flags.focus().await?;
        page.keyboard().press("ArrowRight", None).await?;
        focused(keys.clone(), true).await?;
        expect(keys).to_have_attribute("aria-selected","true").await?;
        page.keyboard().press("End", None).await?;
        focused(page.get_by_role(Tab,named("Users")), true).await?;
        page.keyboard().press("ArrowLeft", None).await?;
        focused(docs.clone(), true).await?;
        expect(docs.clone()).to_have_attribute("aria-selected","true").await?;
        expect(page.get_by_label("Language",true)).to_have_attribute("value","python").await?;
        page.get_by_label("Language",true).click(None).await?;
        texts(page.locator("#dropdown-options").get_by_role(ListOption,None),&["Python","Rust","Go","Node.js"]).await?;
        expect(page.locator("#dropdown-options").get_by_role(ListOption,named("Python"))).to_be_visible().await?;
        page.keyboard().press("Escape", None).await?;
        let copy = page.get_by_role(Button,named("Copy code"));
        expect(copy.clone()).to_be_enabled().await?;
        expect(copy.clone()).to_have_attribute("title","Copy code").await?;
        for (language,label,filename) in [("python","Python","quickstart.py"),("rust","Rust","quickstart.rs"),("go","Go","main.go"),("node","Node.js","quickstart.js")] {
            if language != "python" { choose(page,page.get_by_label("Language",true),language).await?; }
            expect(copy.clone()).to_be_enabled().await?;
            if language != "python" { focused(page.get_by_label("Language",true), true).await?; }
            expect(page.locator("#docs-filename")).to_have_text(filename).await?;
            expect(page.get_by_label(&format!("Complete {label} example"),true)).to_be_visible().await?;
            let code = fs::read_to_string(root().join("examples").join(language).join(filename))?;
            assert_eq!(page.locator("#docs-code").text_content().await?.as_deref(),Some(code.as_str()));
            for kind in ["keyword","comment","string"] { expect(page.locator(format!("#docs-code .token-{kind}")).first()).to_be_visible().await?; }
            copy.click(None).await?;
            expect(page.locator(".code-heading").get_by_role(Status,None)).to_have_text("Copied").await?;
            assert_eq!(js(page,"window.partlyCopied").await?,code);
        }
        page.set_viewport_size(Viewport{width:390,height:844}).await?;
        assert_eq!(js(page,"document.documentElement.scrollWidth <= innerWidth").await?,true);
        docs.focus().await?;
        page.keyboard().press("Home", None).await?;
        focused(flags, true).await?;
        page.keyboard().press("ArrowLeft", None).await?;
        focused(page.get_by_role(Tab,named("Users")), true).await?;
        page.keyboard().press("ArrowLeft", None).await?;
        expect(docs).to_have_attribute("aria-selected","true").await?;
        expect(page.get_by_role(Tabpanel,named("Feature flags"))).to_be_hidden().await?;
        Ok(())
    }).await
}

#[tokio::test]
async fn docs_highlighting_preserves_literal_source_without_html() -> Result {
    let (server, _) = fixture().await?;
    for (language,endpoint,source) in [
        ("python","python.py","# Literal example containing HTML-like data\nAPI_KEY = \"<img src=x onerror=alert(1)>\"\nasync def main():\n    count = 18\n    print(f\"Value: {count}\")\n"),
        ("rust","rust.rs","// Literal source\nfn main() { let value = \"<img src=x onerror=alert(1)>\"; let count = 18; }\n"),
        ("go","go.go","// Literal source\npackage main\nconst value = `<img src=x onerror=alert(1)>`\nconst count = 18\n"),
        ("node","node.js","// Literal source\nconst value = `<img src=x onerror=alert(1)>`;\nconst count = 18;\n"),
    ] {
        run(&format!("docs-literal-{language}"),async |ui| {
            let page = &ui.page;
            page.add_init_script("Object.defineProperty(navigator, 'clipboard', {value: {writeText: async text => { window.partlyCopied = text; }}})").await?;
            page.route(&format!("**/docs/{endpoint}"),move |route| async move { route.fulfill(FulfillOptions::builder().status(200).content_type("text/plain").body_string(source).build()).await }).await?;
            login(page,&server,"docs","development").await?;
            let copy = page.get_by_role(Button,named("Copy code"));
            expect(copy.clone()).to_be_enabled().await?;
            if language != "python" { choose(page,page.get_by_label("Language",true),language).await?; }
            let code = page.locator("#docs-code");
            expect(code.clone()).to_have_text(source).await?;
            assert_eq!(code.text_content().await?.as_deref(),Some(source));
            assert_eq!(code.locator("img, script").count().await?,0);
            for kind in ["comment","string","keyword","number"] { expect(code.locator(format!(".token-{kind}")).first()).to_be_visible().await?; }
            let colors: Vec<String> = code.locator("span").evaluate_all("items => [...new Set(items.map(e => getComputedStyle(e).color))]",None::<()>).await?;
            assert!(colors.len() >= 3);
            copy.click(None).await?;
            expect(page.get_by_role(Status,None)).to_have_text("Copied").await?;
            assert_eq!(js(page,"window.partlyCopied").await?,source);
            Ok(())
        }).await?;
    }
    Ok(())
}

#[tokio::test]
async fn docs_loading_failure_never_copies_a_different_language() -> Result {
    let (server, _) = fixture().await?;
    run("docs-loading", async |ui| {
        let page = &ui.page;
        login(page, &server, "docs", "development").await?;
        let copy = page.get_by_role(Button, named("Copy code"));
        let language = page.get_by_label("Language", true);
        expect(copy.clone()).to_be_enabled().await?;
        let mut held = hold_route(page, "**/docs/rust.rs").await?;
        language.focus().await?;
        for key in ["ArrowDown", "ArrowDown", "Enter"] {
            page.keyboard().press(key, None).await?;
        }
        expect(language.clone()).to_be_disabled().await?;
        expect(copy.clone()).to_be_disabled().await?;
        expect(page.locator("#docs-code"))
            .to_have_text("Loading example…")
            .await?;
        pending(&mut held)
            .await?
            .fulfill(
                FulfillOptions::builder()
                    .status(503)
                    .body_string("Unavailable")
                    .build(),
            )
            .await?;
        expect(page.get_by_role(Alert, None))
            .to_contain_text("Could not load the Rust example")
            .await?;
        expect(language.clone()).to_be_enabled().await?;
        focused(language.clone(), true).await?;
        expect(copy.clone()).to_be_disabled().await?;
        expect(page.locator("#docs-code"))
            .to_have_text("Example unavailable.")
            .await?;
        choose(page, language, "go").await?;
        expect(copy).to_be_enabled().await?;
        expect(page.get_by_role(Alert, None)).to_be_hidden().await?;
        assert_eq!(
            page.locator("#docs-code").text_content().await?.unwrap(),
            fs::read_to_string(root().join("examples/go/main.go"))?
        );
        Ok(())
    })
    .await
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
