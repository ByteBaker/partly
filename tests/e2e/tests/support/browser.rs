// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;
use futures_util::FutureExt;
pub use playwright_rs::{
    expect, expect_page,
    AriaRole::{
        Alert, Button, Checkbox, Combobox, Dialog, Group, Heading, Option as ListOption, Radio,
        Status, Tab, Tablist, Tabpanel,
    },
    FilterOptions, FulfillOptions, GetByRoleOptions, Locator, Page, Viewport,
};
use playwright_rs::{
    BrowserContextOptions, Playwright, ScreenshotOptions, TracingStartOptions, TracingStopOptions,
};
use std::{
    ops::AsyncFnOnce,
    panic::AssertUnwindSafe,
    sync::{Arc, Mutex},
};

pub fn named(name: &str) -> Option<GetByRoleOptions> {
    Some(GetByRoleOptions::default().name(name).exact(true))
}
pub fn containing(name: &str) -> Option<GetByRoleOptions> {
    Some(GetByRoleOptions::default().name(name))
}

pub struct BrowserTest {
    pub page: Page,
    pub requests: Arc<Mutex<Vec<(String, String)>>>,
    pub dialogs: Arc<Mutex<Vec<String>>>,
}

// Keep traces only for failures; always close the browser, including after a panic.
pub async fn run(name: &str, test: impl AsyncFnOnce(&BrowserTest) -> Result) -> Result {
    let playwright = Playwright::launch().await?;
    let engine = std::env::var("PARTLY_BROWSER").unwrap_or_else(|_| "chromium".into());
    let browser = match engine.as_str() {
        "chromium" => playwright.chromium().launch().await?,
        "firefox" => playwright.firefox().launch().await?,
        "webkit" => playwright.webkit().launch().await?,
        _ => return Err(format!("Unknown PARTLY_BROWSER: {engine}").into()),
    };
    let mut options = BrowserContextOptions::builder()
        .viewport(Viewport {
            width: 1280,
            height: 1000,
        })
        .ignore_https_errors(true);
    if engine == "chromium" {
        options = options.permissions(vec!["clipboard-read".into(), "clipboard-write".into()]);
    }
    let context = browser.new_context_with_options(options.build()).await?;
    let tracing = context.tracing().await?;
    tracing
        .start(
            TracingStartOptions::default()
                .screenshots(true)
                .snapshots(true),
        )
        .await?;
    let page = context.new_page().await?;
    let errors = Arc::new(Mutex::new(vec![]));
    let captured = errors.clone();
    page.on_pageerror(move |error| {
        captured.lock().unwrap().push(error);
        async { Ok(()) }
    })
    .await?;
    let requests = Arc::new(Mutex::new(vec![]));
    let captured = requests.clone();
    page.on_request(move |request| {
        captured
            .lock()
            .unwrap()
            .push((request.method().to_owned(), request.url().to_owned()));
        async { Ok(()) }
    })
    .await?;
    let dialogs = Arc::new(Mutex::new(vec![]));
    let captured = dialogs.clone();
    page.on_dialog(move |dialog| {
        captured.lock().unwrap().push(dialog.type_().to_owned());
        async move { dialog.dismiss().await }
    })
    .await?;
    let ui = BrowserTest {
        page,
        requests,
        dialogs,
    };
    let result = AssertUnwindSafe(async {
        tokio::time::timeout(Duration::from_secs(120), test(&ui)).await??;
        assert!(
            errors.lock().unwrap().is_empty(),
            "Browser errors: {:?}",
            errors.lock().unwrap()
        );
        assert!(
            ui.dialogs
                .lock()
                .unwrap()
                .iter()
                .all(|kind| kind == "beforeunload"),
            "Unexpected native browser dialog"
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    })
    .catch_unwind()
    .await;
    if !matches!(&result, Ok(Ok(()))) {
        let directory = root().join("test-results").join(&engine).join(name);
        fs::create_dir_all(&directory)?;
        if let Ok(bytes) = ui
            .page
            .screenshot(Some(ScreenshotOptions::builder().full_page(true).build()))
            .await
        {
            let _ = fs::write(directory.join("failure.png"), bytes);
        }
        let _ = tracing
            .stop(
                TracingStopOptions::default()
                    .path(directory.join("trace.zip").display().to_string()),
            )
            .await;
        eprintln!("Browser artifacts: {}", directory.display());
    } else {
        tracing.stop(None).await?;
    }
    browser.close().await?;
    match result {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

pub async fn login(page: &Page, server: &Server, screen: &str, environment: &str) -> Result {
    page.goto(
        &format!("{}/environments/{environment}/{screen}", server.url),
        None,
    )
    .await?;
    sign_in(page, "admin", PASSWORD).await?;
    expect(page.locator("#workspace")).to_be_visible().await?;
    Ok(())
}

pub async fn sign_in(page: &Page, username: &str, password: &str) -> Result {
    page.locator("#auth")
        .get_by_label("Username", true)
        .fill(username, None)
        .await?;
    page.locator("#auth")
        .get_by_label("Password", true)
        .fill(password, None)
        .await?;
    page.get_by_role(Button, named("Sign in"))
        .click(None)
        .await?;
    Ok(())
}

pub async fn choose(page: &Page, trigger: Locator, value: &str) -> Result {
    trigger.click(None).await?;
    let popup = page.locator("#dropdown-options");
    expect(popup.clone()).to_be_visible().await?;
    popup
        .locator(format!("[role='option'][data-value='{value}']"))
        .click(None)
        .await?;
    Ok(())
}

pub async fn active_option(page: &Page, trigger: &Locator, value: &str) -> Result {
    expect(trigger.clone())
        .to_have_attribute_regex("aria-activedescendant", ".+")
        .await?;
    let id = trigger
        .get_attribute("aria-activedescendant")
        .await?
        .unwrap();
    expect(page.locator(format!("#{id}")))
        .to_have_attribute("data-value", value)
        .await?;
    Ok(())
}

pub async fn create_flag(page: &Page, key: &str, mode: &str, percentage: Option<u8>) -> Result {
    page.get_by_role(Button, named("Create flag"))
        .click(None)
        .await?;
    let modal = page.get_by_role(Dialog, named("Create a flag"));
    modal.get_by_label("Flag key", true).fill(key, None).await?;
    choose(
        page,
        modal.get_by_label("Rollout mode for new flag", true),
        mode,
    )
    .await?;
    if let Some(percentage) = percentage {
        modal
            .get_by_label("Percentage for new flag", true)
            .fill(&percentage.to_string(), None)
            .await?;
    }
    modal
        .get_by_role(Button, named("Create flag"))
        .click(None)
        .await?;
    expect(modal).to_be_hidden().await?;
    expect(page.locator(format!(".flag-row[data-key='{key}']")))
        .to_be_visible()
        .await?;
    Ok(())
}

pub async fn ensure_app(page: &Page) -> Result<bool> {
    if page.locator(".app-row").count().await? != 0 {
        return Ok(false);
    }
    page.get_by_role(Button, named("Create app"))
        .click(None)
        .await?;
    let modal = page.get_by_role(Dialog, named("Create app"));
    modal
        .get_by_label("Application name", true)
        .fill("Test application", None)
        .await?;
    for checkbox in modal.get_by_role(Checkbox, None).all().await? {
        checkbox.check(None).await?;
    }
    modal
        .get_by_role(Button, named("Create app"))
        .click(None)
        .await?;
    expect(modal).to_be_hidden().await?;
    expect(page.locator(".app-row")).to_be_visible().await?;
    Ok(true)
}

pub async fn open_key(page: &Page) -> Result {
    ensure_app(page).await?;
    page.get_by_role(Button, named("Create key"))
        .click(None)
        .await?;
    Ok(())
}

pub async fn create_key(page: &Page, name: &str) -> Result {
    if ensure_app(page).await? {
        page.get_by_label("Name for Default", true)
            .fill(name, None)
            .await?;
        page.get_by_role(Button, named("Save name for Default"))
            .click(None)
            .await?;
    } else {
        open_key(page).await?;
        let modal = page.get_by_role(Dialog, named("Generate API Key"));
        modal
            .get_by_label("Key name", true)
            .fill(name, None)
            .await?;
        modal
            .get_by_role(Button, named("Generate key"))
            .click(None)
            .await?;
        expect(modal).to_be_hidden().await?;
    }
    expect(page.get_by_label(&format!("Name for {name}"), true))
        .to_be_visible()
        .await?;
    Ok(())
}

pub async fn confirm_flag(page: &Page, key: &str) -> Result {
    let url = page.url();
    let environment = url
        .split("/environments/")
        .nth(1)
        .unwrap()
        .split('/')
        .next()
        .unwrap();
    let modal = page.locator("#confirm-flag-dialog");
    expect(modal.clone()).to_be_visible().await?;
    modal
        .locator("#confirm-flag-input")
        .fill(&format!("{environment}/{key}"), None)
        .await?;
    modal.locator("#confirm-flag-submit").click(None).await?;
    expect(modal).to_be_hidden().await?;
    Ok(())
}

pub async fn texts(locator: Locator, expected: &[&str]) -> Result {
    expect(locator.clone())
        .to_have_count(expected.len())
        .await?;
    for (index, text) in expected.iter().enumerate() {
        expect(locator.nth(index as i32)).to_have_text(text).await?;
    }
    Ok(())
}

// shortcut: playwright-rs 0.20.1 uses CSS for is_focused, remove when semantic locators work upstream.
pub async fn focused(locator: Locator, expected: bool) -> Result {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let result = locator
            .evaluate::<bool, ()>("e => document.activeElement === e", None)
            .await;
        if matches!(result,Ok(value) if value==expected) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            let active=locator.evaluate::<Value,()>("() => ({tag: document.activeElement.tagName, id: document.activeElement.id, label: document.activeElement.getAttribute('aria-label')})",None).await;
            return Err(format!(
                "Focus assertion failed for {}: {result:?}; active element: {active:?}",
                locator.selector()
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

pub async fn js(page: &Page, expression: &str) -> Result<Value> {
    Ok(page.evaluate::<(), Value>(expression, None).await?)
}
pub async fn geometry(page: &Page, selector: &str) -> Result<Value> {
    Ok(page
        .locator(selector)
        .evaluate_all::<Value, ()>(
            "items => items.map(e => e.getBoundingClientRect().toJSON())",
            None,
        )
        .await?)
}

pub async fn fail_route(page: &Page, pattern: &str, code: u16, message: &str) -> Result {
    let options = FulfillOptions::builder()
        .status(code)
        .json(&json!({"error":message}))?
        .build();
    page.route(pattern, move |route| {
        let options = options.clone();
        async move { route.fulfill(options).await }
    })
    .await?;
    Ok(())
}

pub async fn hold_route(
    page: &Page,
    pattern: &str,
) -> Result<tokio::sync::mpsc::UnboundedReceiver<playwright_rs::Route>> {
    let (send, receive) = tokio::sync::mpsc::unbounded_channel();
    page.route(pattern, move |route| {
        let _ = send.send(route);
        async { Ok(()) }
    })
    .await?;
    Ok(receive)
}

pub async fn pending(
    receive: &mut tokio::sync::mpsc::UnboundedReceiver<playwright_rs::Route>,
) -> Result<playwright_rs::Route> {
    let route = tokio::time::timeout(Duration::from_secs(5), receive.recv())
        .await?
        .ok_or("Route receiver closed")?;
    assert!(
        receive.try_recv().is_err(),
        "Only one request may be pending"
    );
    Ok(route)
}
