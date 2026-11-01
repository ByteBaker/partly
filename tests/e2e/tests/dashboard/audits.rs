// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;

async fn entry(page: &Page, name: &str, action: &str) -> Result<Locator> {
    let row = page
        .locator(".audit-row")
        .filter(FilterOptions::default().has(page.locator(".audit-item").get_by_text(name, true)))
        .filter(FilterOptions::default().has(page.locator("summary").get_by_text(action, true)));
    row.locator("summary").click(None).await?;
    Ok(row)
}

#[tokio::test]
async fn audit_readable_states_creation_and_responsive_layout() -> Result {
    let (server, admin) = fixture().await?;
    let base = "/api/environments/development";
    admin
        .post(
            &format!("{base}/flags"),
            json!({"key":"checkout","mode":"percentage","percentage":0}),
            201,
        )
        .await?;
    let rules = json!({"mode":"custom","action":"enable","groups":[[{"k":"age","op":"ge","v":18},{"k":"email","op":"suffix","v":"<img src=x onerror=\"alert(1)\">"},{"k":"staff","op":"is","v":false}],[{"k":"plan","op":"matches","v":""}]]});
    admin
        .put(&format!("{base}/flags/checkout"), rules.clone(), 200)
        .await?;
    let mut blocked = rules;
    blocked["key"] = "blocked".into();
    blocked["action"] = "disable".into();
    admin.post(&format!("{base}/flags"), blocked, 201).await?;
    let app = admin.app("development", "Storefront", &[]).await?;
    admin
        .put(
            &format!("{base}/apps/{}", text(&app["id"])),
            json!({"name":"Shop","flags":["checkout"]}),
            200,
        )
        .await?;
    let key = &app["keys"][0];
    admin
        .put(
            &format!("/api/keys/{}", text(&key["id"])),
            json!({"name":"Checkout server"}),
            204,
        )
        .await?;
    admin
        .delete(&format!("/api/keys/{}", text(&key["id"])), 204)
        .await?;
    admin.delete(&format!("{base}/flags/blocked"), 204).await?;
    run("audit-states", async |ui| {
        let page = &ui.page;
        page.set_viewport_size(Viewport {
            width: 1440,
            height: 1000,
        })
        .await?;
        login(page, &server, "audits", "development").await?;
        expect(page.locator(".audit-row")).to_have_count(9).await?;
        let created = entry(page, "checkout", "Created").await?;
        texts(created.locator(".audit-label"), &["Created"]).await?;
        expect(created.locator(".audit-value"))
            .to_have_text("0% of users")
            .await?;
        expect(created.locator(".audit-arrow"))
            .to_have_count(0)
            .await?;
        let updated = entry(page, "checkout", "Updated").await?;
        texts(updated.locator(".audit-label"), &["Before", "After"]).await?;
        texts(
            updated.locator(".audit-value"),
            &["0% of users", "Custom rules"],
        )
        .await?;
        texts(
            updated.locator(".audit-condition"),
            &[
                "age ≥ 18",
                "email ends with “<img src=x onerror=\"alert(1)\">”",
                "staff is false",
                "plan matches “”",
            ],
        )
        .await?;
        texts(updated.locator(".audit-join"), &["AND", "AND", "OR"]).await?;
        expect(updated.locator(".audit-state").last())
            .to_contain_text("Enable when a group matches")
            .await?;
        expect(updated.locator(".audit-state").last())
            .to_contain_text("Otherwise disabled.")
            .await?;
        let deleted = entry(page, "blocked", "Deleted").await?;
        texts(deleted.locator(".audit-label"), &["Deleted"]).await?;
        expect(deleted.locator(".audit-state"))
            .to_contain_text("Disable when a group matches")
            .await?;
        expect(deleted.locator(".audit-state"))
            .to_contain_text("Otherwise enabled.")
            .await?;
        expect(deleted.locator(".audit-arrow"))
            .to_have_count(0)
            .await?;
        let created = entry(page, "Storefront", "Created").await?;
        expect(created.locator(".audit-value"))
            .to_have_text("Storefront")
            .await?;
        expect(created.locator(".audit-state"))
            .to_contain_text("No flags selected")
            .await?;
        expect(created.locator(".audit-detail"))
            .not()
            .to_contain_text("Shop")
            .await?;
        let app_updated = entry(page, "Shop", "Updated").await?;
        texts(app_updated.locator(".audit-value"), &["Storefront", "Shop"]).await?;
        expect(app_updated.locator(".audit-flags"))
            .to_have_text("checkout")
            .await?;
        let created = entry(page, "Default", "Created").await?;
        expect(created.locator(".audit-value"))
            .to_have_text("Active")
            .await?;
        expect(created.locator(".audit-state code"))
            .to_have_text(text(&key["masked_secret"]))
            .await?;
        texts(
            entry(page, "Checkout server", "Renamed")
                .await?
                .locator(".audit-value"),
            &["Default", "Checkout server"],
        )
        .await?;
        texts(
            entry(page, "Checkout server", "Revoked")
                .await?
                .locator(".audit-value"),
            &["Active", "Revoked"],
        )
        .await?;
        assert_eq!(
            page.locator(".audit-detail pre, .audit-detail img")
                .count()
                .await?,
            0
        );
        let audits = page.locator("#audits").inner_text().await?;
        assert!(!audits.contains("null") && !audits.contains(text(&key["secret"])));
        let states = updated.locator(".audit-state");
        let left = states.nth(0).bounding_box().await?.unwrap();
        let right = states.nth(1).bounding_box().await?.unwrap();
        assert!((left.width - right.width).abs() < 2.0 && right.x > left.x + left.width);
        let summary = updated.locator("summary").bounding_box().await?.unwrap();
        let timestamp = updated.locator("time").bounding_box().await?.unwrap();
        assert!((summary.x + summary.width - timestamp.x - timestamp.width).abs() <= 9.0);
        for width in [768, 390] {
            page.set_viewport_size(Viewport { width, height: 844 })
                .await?;
            assert_eq!(
                js(page, "document.documentElement.scrollWidth <= innerWidth").await?,
                true
            );
        }
        let left = states.nth(0).bounding_box().await?.unwrap();
        let right = states.nth(1).bounding_box().await?.unwrap();
        assert!((left.x - right.x).abs() < 2.0 && right.y > left.y + left.height);
        Ok(())
    })
    .await
}
