// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::support::*;
use reqwest::Method;
use serde_json::{Value, json};

const TYPES: &str = "/api/environments/development/types";

#[tokio::test]
async fn type_changes_roll_back_when_audit_write_fails() -> Result {
    let (server, admin) = fixture().await?;
    let definition = json!({"kind":"enum","values":["compact","wide"]});
    admin
        .post(TYPES, json!({"name":"Layout","definition":definition}), 201)
        .await?;
    server.database()?.execute_batch("CREATE TRIGGER reject_type_audit BEFORE INSERT ON audits WHEN NEW.item_type = 'type' BEGIN SELECT RAISE(ABORT, 'audit write failure'); END")?;
    admin
        .post(
            TYPES,
            json!({"name":"Uncommitted","definition":definition}),
            500,
        )
        .await?;
    admin.delete(&format!("{TYPES}/Layout"), 500).await?;
    assert_eq!(admin.get(TYPES).await?, json!({"Layout":definition}));
    assert_eq!(
        admin.get("/api/environments/development/audits").await?["audits"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let rows: (i64, i64) = server.database()?.query_row(
        "SELECT count(*), sum(deleted) FROM result_types",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    assert_eq!(rows, (1, 0));
    server
        .database()?
        .execute_batch("DROP TRIGGER reject_type_audit")?;
    admin
        .post(
            TYPES,
            json!({"name":"Uncommitted","definition":definition}),
            201,
        )
        .await?;
    admin.delete(&format!("{TYPES}/Layout"), 204).await?;
    assert_eq!(admin.get(TYPES).await?, json!({"Uncommitted":definition}));
    Ok(())
}

#[tokio::test]
async fn type_schemas_match_http_shape_and_rollout_restrictions() -> Result {
    let (_server, admin) = fixture().await?;
    let document = admin.get("/openapi.json").await?;
    let types = super::openapi::validator(
        &document,
        json!({"$ref":"#/components/schemas/NamedTypeInput"}),
    )?;
    for (definition, expected) in [
        (json!({"kind":"enum","values":["a"],"extra":1}), 422),
        (
            json!({"kind":"record","fields":{"name":"string"},"extra":1}),
            422,
        ),
        (json!({"kind":"enum","values":[]}), 400),
        (
            json!({"kind":"enum","values":(0..65).map(|n| n.to_string()).collect::<Vec<_>>()}),
            400,
        ),
        (json!({"kind":"record","fields":{}}), 400),
        (
            json!({"kind":"record","fields":(0..33).map(|n| (format!("field{n}"), "string")).collect::<std::collections::BTreeMap<_, _>>()}),
            400,
        ),
    ] {
        let input = json!({"name":"Rejected","definition":definition});
        admin.post(TYPES, input.clone(), expected).await?;
        assert!(
            !types.is_valid(&input),
            "Schema accepted a rejected shape: {input}"
        );
    }
    for definition in [
        json!({"kind":"enum","values":["a"]}),
        json!({"kind":"record","fields":{"name":"string"}}),
    ] {
        assert!(types.is_valid(&json!({"name":"Accepted","definition":definition})));
    }
    let flags =
        super::openapi::validator(&document, json!({"$ref":"#/components/schemas/SavedFlag"}))?;
    let path = "/api/environments/development/flags";
    let mut flag = json!({"key":"typed","mode":"typed","result_type":"string","selected":"on","other":"off","rollout":{"mode":"typed","result_type":"integer","selected":1,"other":0,"rollout":{"mode":"enabled"}}});
    admin.post(path, flag.clone(), 400).await?;
    assert!(
        !flags.is_valid(&flag),
        "Schema accepted a nested typed rollout"
    );
    for (index, rollout) in [
        json!({"mode":"enabled"}),
        json!({"mode":"disabled"}),
        json!({"mode":"percentage","percentage":50}),
        json!({"mode":"custom","action":"enable","groups":[[{"k":"active","op":"is","v":true}]]}),
    ]
    .into_iter()
    .enumerate()
    {
        flag["key"] = json!(format!("typed{index}"));
        flag["rollout"] = rollout;
        assert!(flags.is_valid(&flag));
        let saved = admin.post(path, flag.clone(), 201).await?;
        assert!(flags.is_valid(&saved));
    }
    Ok(())
}

#[tokio::test]
async fn named_types_are_scoped_immutable_and_audited_with_atomic_validation() -> Result {
    let (mut server, admin) = fixture().await?;
    let definition = json!({"kind":"enum","values":["compact","wide"]});
    let input = json!({"name":"Layout","definition":definition});
    admin.post(TYPES, input.clone(), 201).await?;
    admin.post(TYPES, input.clone(), 409).await?;
    let record = json!({"name":"Settings","definition":{"kind":"record","fields":{"layout":"Layout","limit":"integer"}}});
    admin.post(TYPES, record.clone(), 201).await?;
    for definition in [
        json!({"kind":"enum","values":[]}),
        json!({"kind":"enum","values":["a","a"]}),
        json!({"kind":"record","fields":{"nested":"Settings"}}),
        json!({"kind":"record","fields":{"other":"Unknown"}}),
        json!({"kind":"record","fields":{"constructor":"string"}}),
    ] {
        admin
            .post(
                TYPES,
                json!({"name":"Invalid","definition":definition}),
                400,
            )
            .await?;
    }
    assert_eq!(
        admin.get(TYPES).await?,
        json!({"Layout":definition,"Settings":record["definition"]})
    );
    assert_eq!(
        admin.get("/api/environments/production/types").await?,
        json!({})
    );
    admin.delete(&format!("{TYPES}/Layout"), 409).await?;
    let flag = json!({"mode":"typed","result_type":"Settings","selected":{"layout":"wide","limit":20},"other":{"layout":"compact","limit":5},"rollout":{"mode":"enabled"}});
    admin.flag("development", "checkout", flag.clone()).await?;
    admin.delete(&format!("{TYPES}/Settings"), 409).await?;
    let flags = "/api/environments/development/flags";
    for result in [
        json!({"layout":"unknown","limit":5}),
        json!({"layout":"wide","limit":true}),
        json!({"layout":"wide"}),
        json!({"layout":"wide","limit":5,"extra":1}),
    ] {
        let mut changed = flag.clone();
        changed["selected"] = result;
        admin
            .put(&format!("{flags}/checkout"), changed, 400)
            .await?;
    }
    admin
        .put(&format!("{flags}/checkout"), json!({"mode":"enabled"}), 400)
        .await?;
    assert_eq!(admin.get(flags).await?["checkout"], flag);
    let audits = admin.get("/api/environments/development/audits").await?;
    assert_eq!(audits["audits"].as_array().unwrap().len(), 3);
    assert_eq!(audits["audits"][2]["item_type"], "type");
    assert_eq!(audits["audits"][2]["details"]["after"], definition);
    server.stop().await?;
    server.start().await?;
    let admin = server.login("admin", PASSWORD).await?;
    assert_eq!(admin.get(TYPES).await?["Settings"], record["definition"]);
    admin.delete(&format!("{flags}/checkout"), 204).await?;
    admin.delete(&format!("{TYPES}/Settings"), 204).await?;
    admin.delete(&format!("{TYPES}/Layout"), 204).await?;
    admin.post(TYPES, input, 409).await?;
    assert_eq!(
        server
            .database()?
            .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
        1
    );
    Ok(())
}

#[tokio::test]
async fn result_type_permissions_and_policy_scope_preserve_legacy_clients() -> Result {
    let (server, admin) = fixture().await?;
    let definition = json!({"kind":"enum","values":["compact","wide"]});
    let mut editor = None;
    for role in ["viewer", "editor", "manager"] {
        admin.user(role, json!({"development":role}), false).await?;
        let user = server.login(role, PASSWORD).await?;
        if role == "editor" {
            editor = Some(user.clone());
        }
        if role != "manager" {
            user.delete(&format!("{TYPES}/Layout"), 403).await?;
        }
        assert_eq!(user.get(TYPES).await?, json!({}));
        user.post(
            TYPES,
            json!({"name":"Layout","definition":definition}),
            if role == "manager" { 201 } else { 403 },
        )
        .await?;
        status(
            user.request(Method::GET, "/api/environments/production/types")
                .send()
                .await?,
            403,
        )
        .await?;
    }
    admin
        .flag("development", "boolean", json!({"mode":"enabled"}))
        .await?;
    let old_app = admin.app("development", "Legacy app", &["boolean"]).await?;
    let old_key = app_key(&old_app, "development");
    let api = server.api(false)?;
    let old = api
        .request(Method::GET, "/api/policy")
        .headers(policy_headers(&old_key))
        .send()
        .await?;
    let tag = old.headers()["etag"].clone();
    let policy = status(old, 200).await?;
    assert_eq!(policy["schema_version"], 1);
    assert!(policy.get("types").is_none());
    admin
        .post(
            TYPES,
            json!({"name":"Settings","definition":{"kind":"record","fields":{"layout":"Layout"}}}),
            201,
        )
        .await?;
    let typed = json!({"mode":"typed","result_type":"Settings","selected":{"layout":"wide"},"other":{"layout":"compact"},"rollout":{"mode":"enabled"}});
    let editor = editor.unwrap();
    editor.flag("development", "config", typed).await?;
    let app = admin.app("development", "Typed app", &["config"]).await?;
    let key = app_key(&app, "development");
    admin
        .post(
            TYPES,
            json!({"name":"Unrelated","definition":definition}),
            201,
        )
        .await?;
    assert_eq!(
        api.request(Method::GET, "/api/policy")
            .headers(policy_headers(&old_key))
            .header("If-None-Match", tag)
            .send()
            .await?
            .status(),
        304
    );
    let policy = status(
        api.request(Method::GET, "/api/policy")
            .headers(policy_headers(&key))
            .send()
            .await?,
        200,
    )
    .await?;
    assert_eq!(policy["schema_version"], 2);
    assert_eq!(
        policy["flags"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        ["config"]
    );
    assert_eq!(
        policy["types"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        ["Layout", "Settings"]
    );
    // API Keys remain read-only and cannot fetch the environment-wide type catalogue.
    status(
        api.request(Method::GET, TYPES)
            .headers(policy_headers(&key))
            .send()
            .await?,
        401,
    )
    .await?;
    admin
        .put(
            &format!("/api/environments/development/apps/{}", text(&app["id"])),
            json!({"name":"Typed app","flags":["boolean"]}),
            200,
        )
        .await?;
    let policy: Value = status(
        api.request(Method::GET, "/api/policy")
            .headers(policy_headers(&key))
            .send()
            .await?,
        200,
    )
    .await?;
    assert_eq!(policy["schema_version"], 1);
    assert!(policy.get("types").is_none());
    Ok(())
}
