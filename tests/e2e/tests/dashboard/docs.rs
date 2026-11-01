// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;
use std::fs;

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
    for (language, endpoint, source) in [
        (
            "python",
            "python.py",
            "# Literal example containing HTML-like data\nAPI_KEY = \"<img src=x onerror=alert(1)>\"\nasync def main():\n    count = 18\n    print(f\"Value: {count}\")\n",
        ),
        (
            "rust",
            "rust.rs",
            "// Literal source\nfn main() { let value = \"<img src=x onerror=alert(1)>\"; let count = 18; }\n",
        ),
        (
            "go",
            "go.go",
            "// Literal source\npackage main\nconst value = `<img src=x onerror=alert(1)>`\nconst count = 18\n",
        ),
        (
            "node",
            "node.js",
            "// Literal source\nconst value = `<img src=x onerror=alert(1)>`;\nconst count = 18;\n",
        ),
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
