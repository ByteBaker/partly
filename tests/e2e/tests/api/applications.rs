// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::support::*;
use reqwest::Method;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

#[tokio::test]
async fn app_selection_keys_and_audits() -> Result {
    let (server, admin) = fixture().await?;
    for name in ["shared", "billing_only"] {
        admin
            .flag("development", name, json!({"mode":"enabled"}))
            .await?;
    }
    let billing = admin
        .app("development", "Billing", &["shared", "billing_only"])
        .await?;
    let shop = admin.app("development", "Storefront", &["shared"]).await?;
    let empty = admin.app("development", "Empty", &[]).await?;
    let production = admin.app("production", "Billing", &[]).await?;
    assert_ne!(production["id"], billing["id"]);
    let sdk = server.api(false)?;
    assert_eq!(
        status(
            sdk.request(Method::GET, "/api/policy")
                .headers(policy_headers(&app_key(&empty, "development")))
                .send()
                .await?,
            200
        )
        .await?["flags"],
        json!({})
    );
    let first = sdk
        .request(Method::GET, "/api/policy")
        .headers(policy_headers(&app_key(&shop, "development")))
        .send()
        .await?;
    let etag = first.headers()["etag"].clone();
    let first = status(first, 200).await?;
    assert_eq!(first["flags"], json!({"shared":{"mode":"enabled"}}));
    assert_eq!(first["application_id"], shop["id"]);
    assert_eq!(
        status(
            sdk.request(Method::GET, "/api/policy")
                .headers(policy_headers(&app_key(&billing, "development")))
                .send()
                .await?,
            200
        )
        .await?["flags"]
            .as_object()
            .unwrap()
            .len(),
        2
    );
    let mut wrong = app_key(&shop, "development");
    wrong["application_id"] = billing["id"].clone();
    for key in [wrong, app_key(&shop, "production")] {
        assert_eq!(
            sdk.request(Method::GET, "/api/policy")
                .headers(policy_headers(&key))
                .send()
                .await?
                .status(),
            401
        );
    }
    assert_eq!(
        sdk.request(Method::GET, "/api/policy")
            .bearer_auth(text(&shop["keys"][0]["secret"]))
            .send()
            .await?
            .status(),
        401
    );
    admin
        .put(
            "/api/environments/development/flags/billing_only",
            json!({"mode":"disabled"}),
            200,
        )
        .await?;
    assert_eq!(
        sdk.request(Method::GET, "/api/policy")
            .headers(policy_headers(&app_key(&shop, "development")))
            .header("If-None-Match", &etag)
            .send()
            .await?
            .status(),
        304
    );
    admin
        .put(
            &format!("/api/environments/development/apps/{}", text(&shop["id"])),
            json!({"name":"Storefront","flags":[]}),
            200,
        )
        .await?;
    assert_eq!(
        status(
            sdk.request(Method::GET, "/api/policy")
                .headers(policy_headers(&app_key(&shop, "development")))
                .header("If-None-Match", etag)
                .send()
                .await?,
            200
        )
        .await?["flags"],
        json!({})
    );
    let replacement = admin
        .post(
            &format!(
                "/api/environments/development/apps/{}/keys",
                text(&billing["id"])
            ),
            json!({"name":"Replacement"}),
            200,
        )
        .await?;
    let old = &billing["keys"][0];
    admin
        .delete(&format!("/api/keys/{}", text(&old["id"])), 204)
        .await?;
    let revoked = sdk
        .request(Method::GET, "/api/policy")
        .headers(policy_headers(&app_key(&billing, "development")))
        .header("If-None-Match", "*")
        .send()
        .await?;
    assert_eq!(revoked.headers()["x-partly-error"], "key_revoked");
    assert_eq!(status(revoked, 401).await?["code"], "key_revoked");
    let mut new = app_key(&billing, "development");
    new["secret"] = replacement["secret"].clone();
    assert_eq!(
        sdk.request(Method::GET, "/api/policy")
            .headers(policy_headers(&new))
            .send()
            .await?
            .status(),
        200
    );
    let apps = admin.get("/api/environments/development/apps").await?;
    let retained = &apps["applications"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["id"] == billing["id"])
        .unwrap()["keys"][0];
    assert!(retained["secret"].is_null() && !retained["revoked_at"].is_null());
    let secret = text(&old["secret"]);
    assert!(
        text(&retained["masked_secret"]).starts_with(&secret[..7])
            && text(&retained["masked_secret"]).ends_with(&secret[secret.len() - 4..])
    );
    let (stored, digest): (Option<String>, String) = server.database()?.query_row(
        "SELECT secret,secret_hash FROM sdk_keys WHERE id=?1",
        [text(&old["id"])],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    assert!(stored.is_none());
    assert_eq!(digest, format!("{:x}", Sha256::digest(secret.as_bytes())));
    let audits = admin.get("/api/environments/development/audits").await?["audits"].clone();
    let types: BTreeSet<_> = audits
        .as_array()
        .unwrap()
        .iter()
        .map(|a| text(&a["item_type"]))
        .collect();
    assert_eq!(types, BTreeSet::from(["app", "flag", "api_key"]));
    assert!(audits
        .as_array()
        .unwrap()
        .iter()
        .all(|a| a["actor"] == "admin"));
    assert!(
        !audits.to_string().contains(secret)
            && !audits.to_string().contains(text(&replacement["secret"]))
    );
    let filtered = admin
        .get(&format!(
            "/api/environments/development/audits?action=revoked&application_id={}",
            text(&billing["id"])
        ))
        .await?;
    assert_eq!(filtered["audits"].as_array().unwrap().len(), 1);
    assert_eq!(filtered["audits"][0]["item_id"], old["id"]);
    assert_eq!(
        admin
            .get("/api/environments/production/audits?action=revoked")
            .await?["audits"],
        json!([])
    );
    assert_eq!(
        admin
            .get(&format!(
                "/api/environments/development/audits?before={}",
                audits.as_array().unwrap().last().unwrap()["id"]
            ))
            .await?["audits"],
        json!([])
    );
    assert_eq!(
        sdk.request(Method::GET, "/api/environments/development/audits")
            .send()
            .await?
            .status(),
        401
    );
    Ok(())
}

#[tokio::test]
async fn confirmation_is_exact_and_rejects_stale_impact() -> Result {
    let (_server, admin) = fixture().await?;
    let path = "/api/environments/development/flags/shared";
    admin
        .flag("development", "shared", json!({"mode":"enabled"}))
        .await?;
    let impact = admin.get(&format!("{path}/impact")).await?;
    assert_eq!(impact["applications"], json!([]));
    for confirm in ["", "shared", "production/shared", "development/Shared"] {
        status(
            admin
                .request(Method::PUT, path)
                .header("X-Partly-Confirm", confirm)
                .header("X-Partly-Impact", text(&impact["token"]))
                .json(&json!({"mode":"disabled"}))
                .send()
                .await?,
            400,
        )
        .await?;
    }
    let app = admin.app("development", "Billing", &["shared"]).await?;
    for method in [Method::PUT, Method::DELETE] {
        status(
            admin
                .request(method, path)
                .header("X-Partly-Confirm", "development/shared")
                .header("X-Partly-Impact", text(&impact["token"]))
                .json(&json!({"mode":"disabled"}))
                .send()
                .await?,
            409,
        )
        .await?;
    }
    let impact = admin.get(&format!("{path}/impact")).await?;
    assert_eq!(
        impact["applications"],
        json!([{"id":app["id"],"name":"Billing"}])
    );
    for (mode, expected) in [("disabled", 200), ("enabled", 409)] {
        status(
            admin
                .request(Method::PUT, path)
                .header("X-Partly-Confirm", "development/shared")
                .header("X-Partly-Impact", text(&impact["token"]))
                .json(&json!({"mode":mode}))
                .send()
                .await?,
            expected,
        )
        .await?;
    }
    let audit = admin.get("/api/environments/development/audits").await?["audits"][0].clone();
    assert_eq!(audit["details"]["before"], json!({"mode":"enabled"}));
    assert_eq!(audit["details"]["after"], json!({"mode":"disabled"}));
    assert_eq!(audit["details"]["applications"], impact["applications"]);
    admin.delete(path, 204).await?;
    assert_eq!(
        admin.get("/api/environments/development/apps").await?["applications"][0]["flags"],
        json!([])
    );
    Ok(())
}

#[tokio::test]
async fn invalid_assignment_and_audit_failure_roll_back() -> Result {
    let (server, admin) = fixture().await?;
    let app = admin.app("development", "Billing", &[]).await?;
    let path = format!("/api/environments/development/apps/{}", text(&app["id"]));
    let before = admin.get("/api/environments/development/audits").await?;
    admin
        .put(&path, json!({"name":"Changed","flags":["missing"]}), 400)
        .await?;
    assert_eq!(
        admin.get("/api/environments/development/apps").await?["applications"][0]["name"],
        "Billing"
    );
    assert_eq!(
        admin.get("/api/environments/development/audits").await?,
        before
    );
    admin
        .post(
            "/api/environments/development/apps",
            json!({"name":"Billing","flags":[]}),
            409,
        )
        .await?;
    admin
        .put(
            &path.replace("development", "production"),
            json!({"name":"Billing","flags":[]}),
            404,
        )
        .await?;
    server.database()?.execute_batch("CREATE TRIGGER reject_audit BEFORE INSERT ON audits BEGIN SELECT RAISE(ABORT, 'audit write failure'); END")?;
    admin
        .put(&path, json!({"name":"Changed","flags":[]}), 500)
        .await?;
    admin
        .post(
            "/api/environments/development/flags",
            json!({"key":"test","mode":"enabled"}),
            500,
        )
        .await?;
    admin
        .delete(&format!("/api/keys/{}", text(&app["keys"][0]["id"])), 500)
        .await?;
    let restored =
        admin.get("/api/environments/development/apps").await?["applications"][0].clone();
    assert_eq!(restored["name"], "Billing");
    assert_eq!(restored["keys"][0]["secret"], app["keys"][0]["secret"]);
    assert_eq!(
        admin.get("/api/environments/development/flags").await?,
        json!({})
    );
    assert_eq!(
        admin.get("/api/environments/development/audits").await?,
        before
    );
    Ok(())
}

#[tokio::test]
async fn audit_pagination_noop_and_revocation_survive_restart() -> Result {
    for signal in ["INT", "KILL"] {
        let (mut server, admin) = fixture().await?;
        let app = admin.app("development", "Billing", &[]).await?;
        let key = app_key(&app, "development");
        for index in 0..101 {
            admin
                .post(
                    "/api/environments/development/flags",
                    json!({"key":format!("flag_{index}"),"mode":"enabled"}),
                    201,
                )
                .await?;
        }
        let page = admin.get("/api/environments/development/audits").await?["audits"].clone();
        assert_eq!(page.as_array().unwrap().len(), 100);
        let older = admin
            .get(&format!(
                "/api/environments/development/audits?before={}",
                page[99]["id"]
            ))
            .await?["audits"]
            .clone();
        assert_eq!(older.as_array().unwrap().len(), 3);
        let ids: BTreeSet<_> = page
            .as_array()
            .unwrap()
            .iter()
            .chain(older.as_array().unwrap())
            .map(|a| a["id"].as_i64().unwrap())
            .collect();
        assert_eq!(ids.len(), 103);
        assert_eq!(
            admin
                .request(Method::POST, "/api/environments/development/audits")
                .json(&json!({"actor":"someone_else"}))
                .send()
                .await?
                .status(),
            405
        );
        admin
            .put(
                &format!("/api/environments/development/apps/{}", text(&app["id"])),
                json!({"name":"Billing","flags":[]}),
                200,
            )
            .await?;
        admin
            .put(
                "/api/environments/development/flags/flag_0",
                json!({"mode":"enabled"}),
                200,
            )
            .await?;
        assert_eq!(
            admin.get("/api/environments/development/audits").await?["audits"],
            page
        );
        assert_eq!(
            status(
                server
                    .api(false)?
                    .request(Method::GET, "/api/policy")
                    .headers(policy_headers(&key))
                    .send()
                    .await?,
                200
            )
            .await?["flags"],
            json!({})
        );
        admin
            .delete(&format!("/api/keys/{}", text(&key["id"])), 204)
            .await?;
        let before = admin.get("/api/environments/development/audits").await?;
        server.signal(signal).await?;
        server.start().await?;
        let recovered = server.login("admin", PASSWORD).await?;
        let retained = recovered.get("/api/environments/development/apps").await?["applications"]
            [0]["keys"][0]
            .clone();
        assert!(retained["secret"].is_null() && !retained["revoked_at"].is_null());
        assert_eq!(retained["masked_secret"], key["masked_secret"]);
        assert_eq!(
            recovered
                .get("/api/environments/development/audits")
                .await?,
            before
        );
        assert_eq!(
            status(
                server
                    .api(false)?
                    .request(Method::GET, "/api/policy")
                    .headers(policy_headers(&key))
                    .send()
                    .await?,
                401
            )
            .await?["code"],
            "key_revoked"
        );
    }
    Ok(())
}
