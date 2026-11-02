// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::support::*;
use reqwest::{Method, Response};
use serde_json::{Value, json};
use std::{collections::BTreeSet, process::Command};

pub(super) fn validator(document: &Value, mut schema: Value) -> Result<jsonschema::Validator> {
    schema["components"] = document["components"].clone();
    Ok(jsonschema::options().build(&schema)?)
}

struct Contract {
    document: Value,
    covered: BTreeSet<(String, String)>,
}

impl Contract {
    async fn check(
        &mut self,
        method: &str,
        path: &str,
        response: Response,
        expected: u16,
    ) -> Result<Value> {
        assert_eq!(response.status().as_u16(), expected, "{method} {path}");
        assert_eq!(response.headers()["cache-control"], "no-store");
        let definition = &self.document["paths"][path][method]["responses"][expected.to_string()];
        assert!(
            definition.is_object(),
            "Undocumented response: {method} {path} {expected}"
        );
        let media = response
            .headers()
            .get("content-type")
            .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_owned());
        let bytes = response.bytes().await?;
        self.covered.insert((path.into(), method.into()));
        if expected == 204 || expected == 304 {
            assert!(bytes.is_empty());
            assert!(definition.get("content").is_none());
            return Ok(Value::Null);
        }
        let media = media.expect("Response content type");
        let schema = &definition["content"][&media]["schema"];
        assert!(
            schema.is_object(),
            "Undocumented media type: {method} {path} {media}"
        );
        let body = if media == "application/json" {
            serde_json::from_slice(&bytes)?
        } else {
            json!(std::str::from_utf8(&bytes)?)
        };
        validator(&self.document, schema.clone())?
            .validate(&body)
            .map_err(|e| format!("{method} {path}: {e}"))?;
        Ok(body)
    }
}

#[tokio::test]
async fn exported_and_served_contract_matches_every_api_operation() -> Result {
    let mut server = Server::new()?;
    let output = Command::new(binary())
        .arg("openapi")
        .current_dir(server.directory.path())
        .output()?;
    assert!(output.status.success());
    assert!(!server.db.exists());
    assert_eq!(std::fs::read_dir(server.directory.path())?.count(), 0);
    let exported_json = String::from_utf8(output.stdout)?;
    assert_eq!(
        exported_json.lines().count(),
        1,
        "Export must be compact JSON"
    );
    let document: Value = serde_json::from_str(&exported_json)?;
    let committed: Value = serde_json::from_str(include_str!("../../../../docs/openapi.json"))?;
    assert_eq!(document, committed);
    assert_eq!(document["openapi"], "3.1.0");
    let exported = server.directory.path().join("contract.json");
    let output = Command::new(binary())
        .args(["openapi", "--output"])
        .arg(&exported)
        .current_dir(server.directory.path())
        .output()?;
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(std::fs::read_to_string(&exported)?, exported_json);
    assert_eq!(include_str!("../../../../docs/openapi.json"), exported_json);
    assert!(!server.db.exists());
    let failure = Command::new(binary())
        .args(["openapi", "--output", "missing/contract.json"])
        .current_dir(server.directory.path())
        .output()?;
    assert!(!failure.status.success());
    assert!(String::from_utf8(failure.stderr)?.contains("OpenAPI export failed"));
    assert!(!server.db.exists());
    for name in document["components"]["schemas"]
        .as_object()
        .unwrap()
        .keys()
    {
        validator(
            &document,
            json!({"$ref": format!("#/components/schemas/{name}")}),
        )?;
    }
    server.start().await?;
    let public = server.api(false)?;
    let response = public.request(Method::GET, "/openapi.json").send().await?;
    assert_eq!(response.status(), 200);
    assert_eq!(response.headers()["content-type"], "application/json");
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(response.text().await?, exported_json.trim_end());
    assert!(!server.db.exists());
    let mut contract = Contract {
        document,
        covered: BTreeSet::new(),
    };
    let admin = server.api(true)?;
    contract
        .check(
            "get",
            "/api/status",
            public.request(Method::GET, "/api/status").send().await?,
            200,
        )
        .await?;
    let credentials = json!({"username":"admin", "password":PASSWORD});
    let setup = admin
        .request(Method::POST, "/api/setup")
        .json(&credentials)
        .send()
        .await?;
    assert!(setup.headers()["set-cookie"].to_str()?.contains("HttpOnly"));
    contract.check("post", "/api/setup", setup, 200).await?;
    contract
        .check(
            "post",
            "/api/login",
            admin
                .request(Method::POST, "/api/login")
                .json(&credentials)
                .send()
                .await?,
            200,
        )
        .await?;

    let user_input = json!({"username":"viewer", "password":PASSWORD, "is_admin":false, "enabled":true, "permissions":{"development":"viewer"}});
    let user = contract
        .check(
            "post",
            "/api/users",
            admin
                .request(Method::POST, "/api/users")
                .json(&user_input)
                .send()
                .await?,
            201,
        )
        .await?;
    contract
        .check(
            "put",
            "/api/users/{id}",
            admin
                .request(Method::PUT, &format!("/api/users/{}", user["id"]))
                .json(&user_input)
                .send()
                .await?,
            200,
        )
        .await?;
    for path in ["/api/users", "/api/environments"] {
        contract
            .check(
                "get",
                path,
                admin.request(Method::GET, path).send().await?,
                200,
            )
            .await?;
    }
    let types = "/api/environments/development/types";
    let types_template = "/api/environments/{env}/types";
    contract.check("post", types_template, admin.request(Method::POST, types).json(&json!({"name":"Layout","definition":{"kind":"enum","values":["compact","wide"]}})).send().await?, 201).await?;
    contract
        .check(
            "get",
            types_template,
            admin.request(Method::GET, types).send().await?,
            200,
        )
        .await?;
    contract
        .check(
            "delete",
            "/api/environments/{env}/types/{name}",
            admin
                .request(Method::DELETE, &format!("{types}/Layout"))
                .send()
                .await?,
            204,
        )
        .await?;
    let flags = "/api/environments/development/flags";
    let flag_template = "/api/environments/{env}/flags";
    contract
        .check(
            "post",
            flag_template,
            admin
                .request(Method::POST, flags)
                .json(&json!({"key":"checkout", "mode":"percentage", "percentage":50}))
                .send()
                .await?,
            201,
        )
        .await?;
    contract
        .check(
            "get",
            flag_template,
            admin.request(Method::GET, flags).send().await?,
            200,
        )
        .await?;
    let apps = "/api/environments/development/apps";
    let app_template = "/api/environments/{env}/apps";
    let app = contract
        .check(
            "post",
            app_template,
            admin
                .request(Method::POST, apps)
                .json(&json!({"name":"Store", "flags":["checkout"]}))
                .send()
                .await?,
            201,
        )
        .await?;
    let app_path = format!("{apps}/{}", text(&app["id"]));
    contract
        .check(
            "put",
            "/api/environments/{env}/apps/{id}",
            admin
                .request(Method::PUT, &app_path)
                .json(&json!({"name":"Shop", "flags":["checkout"]}))
                .send()
                .await?,
            200,
        )
        .await?;
    let viewer = server.login("viewer", PASSWORD).await?;
    let masked = contract
        .check(
            "get",
            app_template,
            viewer.request(Method::GET, apps).send().await?,
            200,
        )
        .await?;
    assert!(masked["applications"][0]["keys"][0]["secret"].is_null());
    let new_key = contract
        .check(
            "post",
            "/api/environments/{env}/apps/{id}/keys",
            admin
                .request(Method::POST, &format!("{app_path}/keys"))
                .json(&json!({"name":"Rotation"}))
                .send()
                .await?,
            200,
        )
        .await?;
    contract
        .check(
            "put",
            "/api/keys/{id}",
            admin
                .request(Method::PUT, &format!("/api/keys/{}", text(&new_key["id"])))
                .json(&json!({"name":"Next"}))
                .send()
                .await?,
            204,
        )
        .await?;

    let impact_path = format!("{flags}/checkout/impact");
    let impact = contract
        .check(
            "get",
            "/api/environments/{env}/flags/{key}/impact",
            admin.request(Method::GET, &impact_path).send().await?,
            200,
        )
        .await?;
    let changed = json!({"mode":"custom", "action":"enable", "groups":[[{"k":"age", "op":"ge", "v":18}, {"k":"active", "op":"is", "v":true}]]});
    contract
        .check(
            "put",
            "/api/environments/{env}/flags/{key}",
            admin
                .request(Method::PUT, &format!("{flags}/checkout"))
                .header("X-Partly-Confirm", "development/checkout")
                .header("X-Partly-Impact", text(&impact["token"]))
                .json(&changed)
                .send()
                .await?,
            200,
        )
        .await?;
    let key = app_key(&app, "development");
    let policy = public
        .request(Method::GET, "/api/policy")
        .headers(policy_headers(&key))
        .send()
        .await?;
    let etag = policy.headers()["etag"].clone();
    contract.check("get", "/api/policy", policy, 200).await?;
    contract
        .check(
            "get",
            "/api/policy",
            public
                .request(Method::GET, "/api/policy")
                .headers(policy_headers(&key))
                .header("If-None-Match", &etag)
                .send()
                .await?,
            304,
        )
        .await?;
    contract
        .check(
            "post",
            "/api/usage",
            public
                .request(Method::POST, "/api/usage")
                .headers(policy_headers(&key))
                .json(&json!({"flags":["checkout"]}))
                .send()
                .await?,
            204,
        )
        .await?;
    for (path, template) in [
        (
            "/api/environments/development/usage",
            "/api/environments/{env}/usage",
        ),
        (
            "/api/environments/development/audits?before=99999&action=updated",
            "/api/environments/{env}/audits",
        ),
    ] {
        contract
            .check(
                "get",
                template,
                admin.request(Method::GET, path).send().await?,
                200,
            )
            .await?;
    }
    contract
        .check(
            "delete",
            "/api/keys/{id}",
            admin
                .request(
                    Method::DELETE,
                    &format!("/api/keys/{}", text(&app["keys"][0]["id"])),
                )
                .send()
                .await?,
            204,
        )
        .await?;
    for (method, path) in [(Method::GET, "/api/policy"), (Method::POST, "/api/usage")] {
        let mut request = public
            .request(method.clone(), path)
            .headers(policy_headers(&key));
        if method == Method::POST {
            request = request.json(&json!({"flags":["checkout"]}));
        }
        let response = request.send().await?;
        assert_eq!(response.headers()["x-partly-error"], "key_revoked");
        let body = contract
            .check(&method.as_str().to_ascii_lowercase(), path, response, 401)
            .await?;
        assert_eq!(body["code"], "key_revoked");
    }
    let revoked = contract
        .check(
            "get",
            app_template,
            admin.request(Method::GET, apps).send().await?,
            200,
        )
        .await?;
    assert!(
        revoked["applications"][0]["keys"]
            .as_array()
            .unwrap()
            .iter()
            .any(|k| !k["revoked_at"].is_null() && k["secret"].is_null())
    );
    let impact = admin.get(&impact_path).await?;
    contract
        .check(
            "delete",
            "/api/environments/{env}/flags/{key}",
            admin
                .request(Method::DELETE, &format!("{flags}/checkout"))
                .header("X-Partly-Confirm", "development/checkout")
                .header("X-Partly-Impact", text(&impact["token"]))
                .send()
                .await?,
            204,
        )
        .await?;
    contract
        .check(
            "post",
            "/api/logout",
            admin.request(Method::POST, "/api/logout").send().await?,
            204,
        )
        .await?;
    contract
        .check(
            "get",
            "/api/users",
            admin.request(Method::GET, "/api/users").send().await?,
            401,
        )
        .await?;
    // Axum's extractor errors remain plain text; application errors remain JSON.
    contract
        .check(
            "post",
            "/api/login",
            admin
                .request(Method::POST, "/api/login")
                .header("content-type", "application/json")
                .body("{")
                .send()
                .await?,
            400,
        )
        .await?;
    let expected: BTreeSet<_> = contract.document["paths"]
        .as_object()
        .unwrap()
        .iter()
        .flat_map(|(path, operations)| {
            operations
                .as_object()
                .unwrap()
                .keys()
                .map(move |method| (path.clone(), method.clone()))
        })
        .collect();
    assert_eq!(contract.covered, expected);
    Ok(())
}

#[test]
fn schemas_reject_wrong_rule_types_unknown_fields_and_oversized_batches() -> Result {
    let document: Value = serde_json::from_str(include_str!("../../../../docs/openapi.json"))?;
    let flag = validator(&document, json!({"$ref":"#/components/schemas/SavedFlag"}))?;
    for value in [
        json!({"key":"a", "mode":"enabled"}),
        json!({"key":"a", "mode":"disabled"}),
        json!({"key":"a", "mode":"percentage", "percentage":100}),
    ] {
        assert!(flag.is_valid(&value));
    }
    for value in [
        json!({"key":"a", "mode":"enabled", "unknown":1}),
        json!({"key":"a", "mode":"percentage", "percentage":101}),
        json!({"key":"a", "mode":"custom", "action":"enable", "groups":[[]]}),
    ] {
        assert!(!flag.is_valid(&value), "Accepted {value}");
    }
    let condition = validator(&document, json!({"$ref":"#/components/schemas/Condition"}))?;
    for (operators, valid, invalid) in [
        (&["le", "lt", "ge", "gt", "eq"][..], json!(18), json!("18")),
        (
            &["prefix", "suffix", "contains", "matches", "regex"][..],
            json!("text"),
            json!(true),
        ),
        (&["is"][..], json!(false), json!(0)),
    ] {
        for op in operators {
            assert!(condition.is_valid(&json!({"k":"field", "op":op, "v":valid})));
            assert!(!condition.is_valid(&json!({"k":"field", "op":op, "v":invalid})));
        }
    }
    let create_user = validator(
        &document,
        json!({"$ref":"#/components/schemas/CreateUserInput"}),
    )?;
    let update_user = validator(&document, json!({"$ref":"#/components/schemas/UserInput"}))?;
    let mut account =
        json!({"username":"reader", "is_admin":false, "enabled":true, "permissions":{}});
    assert!(update_user.is_valid(&account));
    assert!(!create_user.is_valid(&account));
    account["password"] = Value::Null;
    assert!(!create_user.is_valid(&account));
    account["password"] = json!(PASSWORD);
    assert!(create_user.is_valid(&account));
    let usage = validator(&document, json!({"$ref":"#/components/schemas/UsageInput"}))?;
    assert!(usage.is_valid(&json!({"flags":[]})));
    assert!(!usage.is_valid(&json!({"flags":vec!["flag";129]})));
    assert!(!usage.is_valid(&json!({"flags":[], "context":{}})));
    Ok(())
}
