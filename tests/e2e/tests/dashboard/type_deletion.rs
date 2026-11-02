// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;

#[tokio::test]
async fn type_deletion_cancel_failure_and_success_keep_keyboard_focus() -> Result {
    let (server, admin) = fixture().await?;
    admin
        .post(
            "/api/environments/development/types",
            json!({"name":"Choice","definition":{"kind":"enum","values":["a","b"]}}),
            201,
        )
        .await?;
    run("type-deletion-focus", async |ui| {
        let page = &ui.page;
        login(page, &server, "types", "development").await?;
        let remove = page.get_by_role(Button, named("Delete Choice"));
        let modal = page.get_by_role(Dialog, named("Delete Choice?"));
        let cancel = modal.get_by_role(Button, named("Cancel"));
        for key in ["Enter", "Escape"] {
            remove.click(None).await?;
            expect(modal.clone()).to_be_visible().await?;
            focused(cancel.clone(), true).await?;
            expect(modal.clone())
                .to_contain_text("Its name stays reserved and cannot be reused.")
                .await?;
            page.keyboard().press(key, None).await?;
            expect(modal.clone()).to_be_hidden().await?;
            focused(remove.clone(), true).await?;
            assert!(
                !ui.requests
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|(method, _)| method == "DELETE")
            );
        }

        fail_route(
            page,
            "**/api/environments/development/types/Choice",
            503,
            "Temporary failure",
        )
        .await?;
        remove.click(None).await?;
        modal
            .get_by_role(Button, named("Delete type"))
            .click(None)
            .await?;
        expect(page.get_by_role(Alert, None))
            .to_have_text("Temporary failure")
            .await?;
        focused(remove.clone(), true).await?;
        expect(remove.clone()).to_be_enabled().await?;

        page.unroute("**/api/environments/development/types/Choice")
            .await?;
        remove.click(None).await?;
        modal
            .get_by_role(Button, named("Delete type"))
            .click(None)
            .await?;
        expect(page.locator(".type-card")).to_have_count(0).await?;
        focused(page.get_by_role(Button, named("Create type")), true).await?;
        expect(page.get_by_role(Status, None))
            .to_have_text("Deleted")
            .await?;
        assert_eq!(
            admin.get("/api/environments/development/types").await?,
            json!({})
        );
        Ok(())
    })
    .await
}
