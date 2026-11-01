// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;

#[tokio::test]
async fn dashboard_setup_flags_keys_and_restart() -> Result {
    let mut server = Server::new()?;
    server.start().await?;
    run("setup-restart",async |ui| {
        let page = &ui.page;
        // Chromium also exercises the system clipboard; the other engines lack this permission.
        if std::env::var("PARTLY_BROWSER").unwrap_or_default()!="chromium" && std::env::var("PARTLY_BROWSER").is_ok() {
            page.add_init_script("Object.defineProperty(navigator, 'clipboard', {value: {writeText: async text => { window.partlyCopied = text; }}})").await?;
        }
        page.goto(&server.url,None).await?;
        expect(page.get_by_role(Button,named("Create admin account"))).to_be_visible().await?;
        expect(page.locator("#password-help")).to_have_text("Use at least 12 characters.").await?;
        expect(page.locator("#password")).to_have_attribute("aria-describedby","password-help").await?;
        assert!(!server.db.exists());
        page.locator("#auth").get_by_label("Username",true).fill("admin",None).await?;
        page.locator("#auth").get_by_label("Password",true).fill(PASSWORD,None).await?;
        page.get_by_role(Button,named("Create admin account")).click(None).await?;
        expect_page(page).to_have_url(&format!("{}/environments/development/flags",server.url)).await?;
        assert!(server.db.exists());
        create_flag(page,"new_checkout","percentage",Some(25)).await?;
        page.get_by_label("Percentage for new_checkout",true).fill("50",None).await?;
        page.get_by_role(Button,named("Save new_checkout")).click(None).await?;
        confirm_flag(page,"new_checkout").await?;
        expect(page.get_by_role(Status,None)).to_contain_text("Saved").await?;
        texts(page.get_by_role(Tab,None),&["Feature flags","Apps & Keys","Audits","Docs","Users"]).await?;
        page.get_by_role(Tab,named("Apps & Keys")).click(None).await?;
        expect_page(page).to_have_url(&format!("{}/environments/development/apps",server.url)).await?;
        let heading = geometry(page,".workspace-heading").await?;
        create_key(page,"Checkout service").await?;
        expect(page.locator(".key-row").get_by_role(Status,None)).to_have_text("Saved").await?;
        assert_eq!(geometry(page,".workspace-heading").await?,heading);
        let field = page.locator(".key-controls input");
        expect(field.clone()).to_have_attribute("type","text").await?;
        expect(field.clone()).to_have_value_regex("^pk_[A-Za-z0-9]{4}•+[A-Za-z0-9]{4}$").await?;
        ui.requests.lock().unwrap().clear();
        let copy = page.get_by_role(Button,containing("Copy API Key "));
        copy.click(None).await?;
        expect(page.get_by_role(Status,None)).to_have_text("Copied").await?;
        expect(page.locator(".key-row").get_by_role(Status,None)).to_have_text("Copied").await?;
        assert_eq!(geometry(page,".workspace-heading").await?,heading);
        expect(copy.clone()).to_have_text("Copy").await?;
        let secret = js(page,"async () => window.partlyCopied ?? await navigator.clipboard.readText()").await?;
        let secret = text(&secret).to_owned();
        assert!(secret.starts_with("pk_") && secret.len()==27 && secret[3..].chars().all(|c|c.is_ascii_alphanumeric()));
        let mask = format!("{}{}{}",&secret[..7],"•".repeat(12),&secret[secret.len()-4..]);
        expect(field.clone()).to_have_value(&mask).await?;
        page.get_by_role(Button,named("Reveal")).click(None).await?;
        expect(field.clone()).to_have_value(&secret).await?;
        page.get_by_role(Button,named("Hide")).click(None).await?;
        expect(field.clone()).to_have_attribute("type","text").await?;
        assert!(ui.requests.lock().unwrap().is_empty());
        assert_eq!(page.locator(".key-row code").count().await?,0);
        page.get_by_label("Name for Checkout service",true).fill("Checkout worker",None).await?;
        page.get_by_role(Button,named("Save name for Checkout service")).click(None).await?;
        expect(page.get_by_label("Name for Checkout worker",true)).to_have_value("Checkout worker").await?;
        expect(page.locator(".key-row").get_by_role(Status,None)).to_have_text("Saved").await?;
        server.stop().await?; server.start().await?;
        page.reload(None).await?;
        sign_in(page,"admin",PASSWORD).await?;
        expect(page.get_by_label("Name for Checkout worker",true)).to_have_value("Checkout worker").await?;
        expect(field.clone()).to_have_attribute("type","text").await?;
        expect(field.clone()).to_have_value(&mask).await?;
        copy.focus().await?; page.keyboard().press("Enter",None).await?;
        expect(page.get_by_role(Status,None)).to_have_text("Copied").await?;
        assert_eq!(js(page,"async () => window.partlyCopied ?? await navigator.clipboard.readText()").await?,secret);
        page.set_viewport_size(Viewport{width:390,height:844}).await?;
        assert_eq!(js(page,"document.documentElement.scrollWidth <= innerWidth").await?,true);
        page.evaluate_expression("() => { navigator.clipboard.writeText = async () => { throw new DOMException('Clipboard blocked', 'NotAllowedError'); }; }").await?;
        copy.click(None).await?;
        expect(page.get_by_role(Alert,None)).to_contain_text("Could not copy").await?;
        expect(field).to_have_attribute("type","text").await?;
        // Let the clipboard error's layout and any queued scroll settle before the next action.
        js(page,"async () => { await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))); }").await?;
        // Locator.click scrolls an already-visible sticky control in some browser drivers.
        let env = page.get_by_label("Environment",true).bounding_box().await?.unwrap();
        page.mouse().click(env.x + env.width / 2.0, env.y + env.height / 2.0, None).await?;
        page.locator("#dropdown-options [data-value=production]").click(None).await?;
        expect_page(page).to_have_url(&format!("{}/environments/production/apps",server.url)).await?;
        expect(page.get_by_text("No applications in this environment.",true)).to_be_visible().await?;
        page.get_by_role(Tab,named("Feature flags")).click(None).await?;
        expect(page.get_by_text("No flags in this environment",true)).to_be_visible().await?;
        choose(page,page.get_by_label("Environment",true),"development").await?;
        expect(page.get_by_label("Percentage for new_checkout",true)).to_have_value("50").await?;
        page.get_by_role(Button,named("Delete new_checkout")).click(None).await?;
        confirm_flag(page,"new_checkout").await?;
        expect(page.get_by_text("No flags in this environment",true)).to_be_visible().await?;
        page.get_by_role(Tab,named("Apps & Keys")).click(None).await?;
        page.get_by_role(Button,named("Revoke")).click(None).await?;
        page.get_by_role(Dialog,named("Revoke API Key?")).get_by_role(Button,named("Revoke key")).click(None).await?;
        expect(page.get_by_text("Revoked keys (1)",true)).to_be_visible().await?;
        page.get_by_role(Button,named("Sign out")).click(None).await?;
        expect(page.get_by_role(Button,named("Sign in"))).to_be_visible().await?;
        expect(page.get_by_role(Tablist,None)).to_be_hidden().await?;
        assert_eq!(page.locator(".key-controls input").count().await?,0);
        Ok(())
    }).await
}

#[tokio::test]
async fn https_dashboard_setup_login_flags_and_keys() -> Result {
    let mut server = Server::new()?;
    let tls = Tls::new("127.0.0.1", false)?;
    tls.configure(&mut server);
    server.start().await?;
    run("https-dashboard", async |ui| {
        let page = &ui.page;
        let writes = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
        let captured = writes.clone();
        page.on_request(move |request| {
            if ["POST", "PUT", "DELETE"].contains(&request.method()) {
                captured.lock().unwrap().push(request);
            }
            async { Ok(()) }
        })
        .await?;
        page.goto(&server.url, None).await?;
        expect(page.get_by_role(Button, named("Create admin account")))
            .to_be_visible()
            .await?;
        assert!(!server.db.exists());
        page.locator("#auth")
            .get_by_label("Username", true)
            .fill("admin", None)
            .await?;
        page.locator("#auth")
            .get_by_label("Password", true)
            .fill(PASSWORD, None)
            .await?;
        page.get_by_role(Button, named("Create admin account"))
            .click(None)
            .await?;
        expect_page(page)
            .to_have_url(&format!("{}/environments/development/flags", server.url))
            .await?;
        assert_eq!(
            js(
                page,
                "performance.getEntriesByType('navigation')[0].nextHopProtocol"
            )
            .await?,
            "h2"
        );
        let cookies = page.context().unwrap().cookies(None).await?;
        let session = cookies
            .iter()
            .find(|cookie| cookie.name == "partly_session")
            .unwrap();
        assert!(session.secure && session.http_only);
        assert_eq!(session.same_site.as_deref(), Some("Strict"));
        create_flag(page, "new_checkout", "enabled", None).await?;
        page.get_by_role(Tab, named("Apps & Keys"))
            .click(None)
            .await?;
        create_key(page, "HTTPS application").await?;
        expect(page.locator(".key-controls input"))
            .to_have_value_regex("^pk_[A-Za-z0-9]{4}•+[A-Za-z0-9]{4}$")
            .await?;
        page.get_by_role(Button, named("Sign out"))
            .click(None)
            .await?;
        expect(page.get_by_role(Button, named("Sign in")))
            .to_be_visible()
            .await?;
        login(page, &server, "apps", "development").await?;
        expect(page.get_by_label("Name for HTTPS application", true))
            .to_be_visible()
            .await?;
        let writes = writes.lock().unwrap().clone();
        assert!(!writes.is_empty());
        for request in writes {
            assert!(request.url().starts_with(&format!("{}/api/", server.url)));
            assert_eq!(
                request.header_value("origin").await?.as_deref(),
                Some(server.url.as_str())
            );
        }
        Ok(())
    })
    .await
}
