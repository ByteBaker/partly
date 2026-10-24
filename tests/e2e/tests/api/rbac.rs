// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::support::*;
use reqwest::Method;
use serde_json::json;

#[tokio::test]
async fn environment_permissions_are_enforced_on_every_api() -> Result {
    for role in ["viewer", "editor", "manager"] {
        let (server, admin) = fixture().await?;
        admin
            .flag("development", "new_checkout", json!({"mode":"enabled"}))
            .await?;
        let key = admin.key("development").await?;
        let prod = admin.key("production").await?;
        let app = admin.get("/api/environments/development/apps").await?["applications"][0].clone();
        let user = admin.user(role, json!({"development":role}), false).await?;
        let client = server.login(role, PASSWORD).await?;
        let path = "/api/environments/development";
        let access = client.get("/api/environments").await?;
        assert_eq!(access["environments"], json!(["development"]));
        assert_eq!(access["user"]["username"], role);
        for resource in ["flags", "apps", "audits", "flags/new_checkout/impact"] {
            client.get(&format!("{path}/{resource}")).await?;
            status(
                client
                    .request(
                        Method::GET,
                        &format!("/api/environments/production/{resource}"),
                    )
                    .send()
                    .await?,
                403,
            )
            .await?;
        }
        let response = client.get(&format!("{path}/apps")).await?;
        assert!(!response.to_string().contains(text(&prod["secret"])));
        assert_eq!(
            response.to_string().contains(text(&key["secret"])),
            role == "manager"
        );
        assert_eq!(
            response["applications"][0]["keys"][0]["masked_secret"],
            app["keys"][0]["masked_secret"]
        );
        status(client.request(Method::GET, "/api/users").send().await?, 403).await?;
        client
            .update_user(&user, json!({"is_admin":true,"permissions":{}}), 403)
            .await?;
        client.post("/api/users", json!({"username":"intruder","password":PASSWORD,"enabled":true,"is_admin":true,"permissions":{}}), 403).await?;
        client
            .put(
                &format!("/api/keys/{}", text(&prod["id"])),
                json!({"name":"stolen"}),
                403,
            )
            .await?;
        client
            .delete(&format!("/api/keys/{}", text(&prod["id"])), 403)
            .await?;
        client
            .post(
                "/api/environments/production/flags",
                json!({"key":"denied","mode":"enabled"}),
                403,
            )
            .await?;
        client
            .post(
                &format!("{path}/flags"),
                json!({"key":"new","mode":"disabled"}),
                if role == "viewer" { 403 } else { 201 },
            )
            .await?;
        client
            .put(
                &format!("{path}/flags/new_checkout"),
                json!({"mode":"disabled"}),
                if role == "viewer" { 403 } else { 200 },
            )
            .await?;
        client
            .delete(
                &format!("{path}/flags/new_checkout"),
                if role == "viewer" { 403 } else { 204 },
            )
            .await?;
        client
            .post(
                &format!("{path}/apps"),
                json!({"name":"Another app","flags":[]}),
                if role == "manager" { 201 } else { 403 },
            )
            .await?;
        client
            .put(
                &format!("{path}/apps/{}", text(&app["id"])),
                json!({"name":"Renamed app","flags":[]}),
                if role == "manager" { 200 } else { 403 },
            )
            .await?;
        client
            .post(
                &format!("{path}/apps/{}/keys", text(&app["id"])),
                json!({"name":"Rotation"}),
                if role == "manager" { 200 } else { 403 },
            )
            .await?;
        client
            .put(
                &format!("/api/keys/{}", text(&key["id"])),
                json!({"name":"Renamed key"}),
                if role == "manager" { 204 } else { 403 },
            )
            .await?;
        client
            .delete(
                &format!("/api/keys/{}", text(&key["id"])),
                if role == "manager" { 204 } else { 403 },
            )
            .await?;
        let entries = client.get(&format!("{path}/audits")).await?;
        assert_eq!(
            entries["audits"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["actor"] == role),
            role != "viewer"
        );
        for secret in [&key["secret"], &prod["secret"]] {
            assert!(!entries.to_string().contains(text(secret)));
        }
        assert_eq!(
            admin.get("/api/environments/production/flags").await?,
            json!({})
        );
    }
    Ok(())
}

#[tokio::test]
async fn unchanged_user_saves_preserve_sessions_and_audits() -> Result {
    let (server, admin) = fixture().await?;
    let initial = admin.get("/api/users").await?["users"][0].clone();
    let user = admin
        .user("teammate", json!({"development":"viewer"}), false)
        .await?;
    let first = server.login("teammate", PASSWORD).await?;
    let second = server.login("teammate", PASSWORD).await?;
    let mut audits = vec![];
    for env in ["development", "production"] {
        audits.push(
            admin
                .get(&format!("/api/environments/{env}/audits"))
                .await?,
        );
    }
    for account in [&user, &initial] {
        assert_eq!(admin.update_user(account, json!({}), 200).await?, *account);
    }
    for client in [&first, &second, &admin] {
        client.get("/api/environments").await?;
    }
    for (env, before) in ["development", "production"].iter().zip(&audits) {
        assert_eq!(
            admin
                .get(&format!("/api/environments/{env}/audits"))
                .await?,
            *before
        );
    }

    // Supplying even the same password is an intentional reset, not an unchanged save.
    admin
        .update_user(&user, json!({"password":PASSWORD}), 200)
        .await?;
    for client in [&first, &second] {
        status(
            client
                .request(Method::GET, "/api/environments")
                .send()
                .await?,
            401,
        )
        .await?;
    }
    for (env, before) in ["development", "production"].iter().zip(&audits) {
        let entries = admin
            .get(&format!("/api/environments/{env}/audits"))
            .await?;
        assert_eq!(
            entries["audits"].as_array().unwrap().len(),
            before["audits"].as_array().unwrap().len() + 1
        );
        assert_eq!(entries["audits"][0]["details"]["password_changed"], true);
    }
    Ok(())
}

#[tokio::test]
async fn permission_password_and_account_changes_end_sessions() -> Result {
    let (mut server, admin) = fixture().await?;
    let user = admin
        .user(
            "teammate",
            json!({"development":"manager","production":"viewer"}),
            false,
        )
        .await?;
    let first = server.login("teammate", PASSWORD).await?;
    let second = server.login("teammate", PASSWORD).await?;
    admin
        .update_user(&user, json!({"permissions":{"production":"viewer"}}), 200)
        .await?;
    for client in [first, second] {
        status(
            client
                .request(Method::GET, "/api/environments")
                .send()
                .await?,
            401,
        )
        .await?;
    }
    let client = server.login("teammate", PASSWORD).await?;
    assert_eq!(
        client.get("/api/environments").await?["environments"],
        json!(["production"])
    );
    status(
        client
            .request(Method::GET, "/api/environments/development/apps")
            .send()
            .await?,
        403,
    )
    .await?;
    admin
        .update_user(&user, json!({"enabled":false}), 200)
        .await?;
    status(
        client
            .request(Method::GET, "/api/environments")
            .send()
            .await?,
        401,
    )
    .await?;
    client
        .post(
            "/api/login",
            json!({"username":"teammate","password":PASSWORD}),
            401,
        )
        .await?;
    let password = "replacement-test-password";
    admin
        .update_user(&user, json!({"password":password}), 200)
        .await?;
    client
        .post(
            "/api/login",
            json!({"username":"teammate","password":PASSWORD}),
            401,
        )
        .await?;
    server.stop().await?;
    server.start().await?;
    let client = server.login("teammate", password).await?;
    assert_eq!(
        client.get("/api/environments").await?["user"]["permissions"],
        user["permissions"]
    );
    let entries = server
        .login("admin", PASSWORD)
        .await?
        .get("/api/environments/development/audits")
        .await?;
    assert!(entries["audits"]
        .as_array()
        .unwrap()
        .iter()
        .all(|entry| entry["actor"] == "admin"));
    assert_eq!(entries["audits"][0]["details"]["password_changed"], true);
    for secret in [PASSWORD, password, "$argon2"] {
        assert!(!entries.to_string().contains(secret));
    }
    Ok(())
}

#[tokio::test]
async fn initial_admin_account_validation_and_delegation() -> Result {
    let (server, admin) = fixture().await?;
    let initial = admin.get("/api/users").await?["users"][0].clone();
    for (changes, code) in [
        (json!({"enabled":false}), 409),
        (json!({"is_admin":false}), 409),
        (json!({"username":"renamed"}), 400),
    ] {
        admin.update_user(&initial, changes, code).await?;
    }
    admin.get("/api/environments").await?;
    let user = admin.user("unassigned", json!({}), false).await?;
    let client = server.login("unassigned", PASSWORD).await?;
    assert_eq!(
        client.get("/api/environments").await?["environments"],
        json!([])
    );
    status(
        client
            .request(Method::GET, "/api/environments/development/flags")
            .send()
            .await?,
        403,
    )
    .await?;
    let mut body = user.clone();
    body.as_object_mut().unwrap().remove("id");
    body["password"] = PASSWORD.into();
    admin.post("/api/users", body.clone(), 409).await?;
    for (changes, code) in [
        (json!({"username":" spaces "}), 400),
        (json!({"username":"n","password":"short"}), 400),
        (json!({"permissions":{"production":"owner"}}), 422),
        (json!({"permissions":{"unknown":"viewer"}}), 400),
        (
            json!({"is_admin":true,"permissions":{"production":"viewer"}}),
            400,
        ),
    ] {
        admin.update_user(&user, changes, code).await?;
    }
    body["username"] = "missing_password".into();
    body.as_object_mut().unwrap().remove("password");
    admin.post("/api/users", body.clone(), 400).await?;
    let mut absent = user.clone();
    absent["id"] = 999.into();
    admin.update_user(&absent, json!({}), 404).await?;
    let anonymous = server.api(true)?;
    status(
        anonymous.request(Method::GET, "/api/users").send().await?,
        401,
    )
    .await?;
    body["password"] = PASSWORD.into();
    anonymous.post("/api/users", body, 401).await?;
    let other = admin.user("other_admin", json!({}), true).await?;
    let client = server.login("other_admin", PASSWORD).await?;
    client.get("/api/users").await?;
    assert_eq!(
        client.get("/api/environments").await?["environments"],
        json!(["development", "production"])
    );
    admin
        .update_user(&other, json!({"is_admin":false}), 200)
        .await?;
    status(client.request(Method::GET, "/api/users").send().await?, 401).await?;
    Ok(())
}

#[tokio::test]
async fn user_change_and_audit_are_atomic() -> Result {
    let (server, admin) = fixture().await?;
    let user = admin
        .user("viewer", json!({"development":"viewer"}), false)
        .await?;
    let client = server.login("viewer", PASSWORD).await?;
    server.database()?.execute_batch("CREATE TRIGGER fail_user_audit BEFORE INSERT ON audits BEGIN SELECT RAISE(ABORT, 'test'); END")?;
    admin
        .update_user(&user, json!({"enabled":false,"permissions":{}}), 500)
        .await?;
    client.get("/api/environments/development/flags").await?;
    assert_eq!(
        admin.get("/api/users").await?["users"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == user["id"]),
        Some(&user)
    );
    Ok(())
}

#[tokio::test]
async fn password_reset_and_disable_preserve_application_keys() -> Result {
    let (server, admin) = fixture().await?;
    let user = admin
        .user("manager", json!({"development":"manager"}), false)
        .await?;
    let client = server.login("manager", PASSWORD).await?;
    let app = client.app("development", "Service", &[]).await?;
    admin
        .update_user(&user, json!({"password":"new-test-password"}), 200)
        .await?;
    status(
        client
            .request(Method::GET, "/api/environments/development/flags")
            .send()
            .await?,
        401,
    )
    .await?;
    let client = server.login("manager", "new-test-password").await?;
    client.get("/api/environments/development/flags").await?;
    admin
        .update_user(&user, json!({"enabled":false}), 200)
        .await?;
    status(
        client
            .request(Method::GET, "/api/environments/development/flags")
            .send()
            .await?,
        401,
    )
    .await?;
    let sdk = server.api(false)?;
    let key = app_key(&app, "development");
    status(
        sdk.request(Method::GET, "/api/policy")
            .headers(policy_headers(&key))
            .send()
            .await?,
        200,
    )
    .await?;
    status(
        sdk.request(Method::GET, "/api/users")
            .headers(policy_headers(&key))
            .send()
            .await?,
        401,
    )
    .await?;
    Ok(())
}
