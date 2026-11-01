// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::{PASSWORD, Result};
use reqwest::{Client, Method, Response, header::HeaderMap};
use serde_json::{Value, json};

pub fn text(value: &Value) -> &str {
    value.as_str().expect("Expected a JSON string")
}

pub async fn status(response: Response, expected: u16) -> Result<Value> {
    let actual = response.status().as_u16();
    let body = response.text().await?;
    assert_eq!(actual, expected, "Unexpected response: {body}");
    if body.is_empty() {
        Ok(Value::Null)
    } else {
        Ok(serde_json::from_str(&body).unwrap_or(Value::String(body)))
    }
}

#[derive(Clone)]
pub struct Api {
    pub client: Client,
    pub url: String,
}

impl Api {
    pub fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.client.request(method, format!("{}{path}", self.url))
    }

    pub async fn get(&self, path: &str) -> Result<Value> {
        status(self.request(Method::GET, path).send().await?, 200).await
    }

    pub async fn post(&self, path: &str, value: Value, expected: u16) -> Result<Value> {
        status(
            self.request(Method::POST, path).json(&value).send().await?,
            expected,
        )
        .await
    }

    pub async fn change(
        &self,
        method: Method,
        path: &str,
        body: Value,
        expected: u16,
    ) -> Result<Value> {
        let mut request = self.request(method, path);
        let parts: Vec<_> = path.split('/').collect();
        if parts.len() == 6 && parts[4] == "flags" {
            let impact = self
                .request(Method::GET, &format!("{path}/impact"))
                .send()
                .await?;
            request = request.header("X-Partly-Confirm", format!("{}/{}", parts[3], parts[5]));
            if impact.status().is_success() {
                request = request.header(
                    "X-Partly-Impact",
                    text(&impact.json::<Value>().await?["token"]),
                );
            }
        }
        status(request.json(&body).send().await?, expected).await
    }

    pub async fn put(&self, path: &str, body: Value, expected: u16) -> Result<Value> {
        self.change(Method::PUT, path, body, expected).await
    }

    pub async fn delete(&self, path: &str, expected: u16) -> Result<Value> {
        self.change(Method::DELETE, path, Value::Null, expected)
            .await
    }

    pub async fn flag(&self, environment: &str, key: &str, flag: Value) -> Result<Value> {
        let path = format!("/api/environments/{environment}/flags");
        let mut body = flag.clone();
        body["key"] = key.into();
        let response = self.request(Method::POST, &path).json(&body).send().await?;
        let saved = if response.status() == 409 {
            self.put(&format!("{path}/{key}"), flag, 200).await?
        } else {
            status(response, 201).await?
        };
        assert_eq!(saved, body);
        // Only the fixture application subscribes automatically; real apps never do.
        for app in self
            .get(&format!("/api/environments/{environment}/apps"))
            .await?["applications"]
            .as_array()
            .unwrap()
        {
            if app["name"] == "Test application"
                && !app["flags"].as_array().unwrap().contains(&json!(key))
            {
                let mut flags = app["flags"].clone();
                flags.as_array_mut().unwrap().push(json!(key));
                self.put(
                    &format!("/api/environments/{environment}/apps/{}", text(&app["id"])),
                    json!({"name":"Test application","flags":flags}),
                    200,
                )
                .await?;
            }
        }
        Ok(saved)
    }

    pub async fn app(&self, environment: &str, name: &str, flags: &[&str]) -> Result<Value> {
        self.post(
            &format!("/api/environments/{environment}/apps"),
            json!({"name":name,"flags":flags}),
            201,
        )
        .await
    }

    pub async fn key(&self, environment: &str) -> Result<Value> {
        let apps = self
            .get(&format!("/api/environments/{environment}/apps"))
            .await?;
        let (app, key) = if let Some(app) = apps["applications"].as_array().unwrap().first() {
            let key = self
                .post(
                    &format!(
                        "/api/environments/{environment}/apps/{}/keys",
                        text(&app["id"])
                    ),
                    json!({"name":"Test application"}),
                    200,
                )
                .await?;
            (app.clone(), key)
        } else {
            let flags = self
                .get(&format!("/api/environments/{environment}/flags"))
                .await?;
            let names: Vec<&str> = flags
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            let app = self.app(environment, "Test application", &names).await?;
            let key = app["keys"][0].clone();
            self.put(
                &format!("/api/keys/{}", text(&key["id"])),
                json!({"name":"Test application"}),
                204,
            )
            .await?;
            (app, key)
        };
        let mut key = key;
        key["application_id"] = app["id"].clone();
        key["environment"] = environment.into();
        Ok(key)
    }

    pub async fn user(&self, name: &str, permissions: Value, is_admin: bool) -> Result<Value> {
        let user = self.post("/api/users", json!({"username":name,"password":PASSWORD,"is_admin":is_admin,"enabled":true,"permissions":permissions}), 201).await?;
        assert!(!user.to_string().contains("password"));
        Ok(user)
    }

    pub async fn update_user(&self, user: &Value, changes: Value, expected: u16) -> Result<Value> {
        let mut body = user.clone();
        body.as_object_mut().unwrap().remove("id");
        body.as_object_mut()
            .unwrap()
            .extend(changes.as_object().unwrap().clone());
        self.put(&format!("/api/users/{}", user["id"]), body, expected)
            .await
    }
}

pub fn policy_headers(key: &Value) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "Authorization",
        format!("Bearer {}", text(&key["secret"])).parse().unwrap(),
    );
    headers.insert(
        "X-Partly-Environment",
        text(&key["environment"]).parse().unwrap(),
    );
    headers.insert(
        "X-Partly-Application",
        text(&key["application_id"]).parse().unwrap(),
    );
    headers
}

pub fn app_key(app: &Value, environment: &str) -> Value {
    let mut key = app["keys"][0].clone();
    key["environment"] = environment.into();
    key["application_id"] = app["id"].clone();
    key
}
