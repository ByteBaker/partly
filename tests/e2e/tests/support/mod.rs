// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use reqwest::{header::HeaderMap, Client, Method, Response};
use serde_json::{json, Value};
use std::{
    fs::{self, File},
    net::TcpListener,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use tempfile::TempDir;

#[cfg(feature = "browser")]
pub mod browser;
pub mod driver;
pub mod http;

pub type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub const PASSWORD: &str = "local-test-password";

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

pub fn binary() -> PathBuf {
    std::env::var_os("PARTLY_BINARY")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("target/debug/partly"))
}

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

pub struct Server {
    pub directory: TempDir,
    pub db: PathBuf,
    pub port: u16,
    pub url: String,
    pub process: Option<Child>,
    pub host: String,
    pub extra: Vec<String>,
    pub ca: Option<PathBuf>,
}

impl Server {
    pub fn new() -> Result<Self> {
        let directory = tempfile::tempdir()?;
        let db = directory.path().join("partly.db");
        let port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
        Ok(Self {
            directory,
            db,
            port,
            url: format!("http://127.0.0.1:{port}"),
            process: None,
            host: "127.0.0.1".into(),
            extra: vec![],
            ca: None,
        })
    }

    pub fn log(&self) -> String {
        fs::read_to_string(self.directory.path().join("server.log")).unwrap_or_default()
    }

    pub fn spawn(&mut self) -> Result {
        let log = File::options()
            .create(true)
            .append(true)
            .open(self.directory.path().join("server.log"))?;
        self.process = Some(
            Command::new(binary())
                .args([
                    "serve",
                    "--host",
                    &self.host,
                    "--port",
                    &self.port.to_string(),
                    "--db",
                ])
                .arg(&self.db)
                .args(&self.extra)
                .current_dir(self.directory.path())
                .stdin(Stdio::null())
                .stdout(log.try_clone()?)
                .stderr(log)
                .spawn()?,
        );
        Ok(())
    }

    pub async fn start(&mut self) -> Result {
        self.spawn()?;
        let api = self.api(false)?;
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if self.process.as_mut().unwrap().try_wait()?.is_some() {
                return Err(self.log().into());
            }
            if let Ok(response) = api
                .request(Method::GET, "/api/status")
                .timeout(Duration::from_millis(300))
                .send()
                .await
            {
                if response.status() == 200 {
                    return Ok(());
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Err(format!("Server did not start: {}", self.log()).into())
    }

    pub async fn start_tcp(&mut self) -> Result {
        self.spawn()?;
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if self.process.as_mut().unwrap().try_wait()?.is_some() {
                return Err(self.log().into());
            }
            if tokio::net::TcpStream::connect(("127.0.0.1", self.port))
                .await
                .is_ok()
            {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        Err(format!("Server did not listen: {}", self.log()).into())
    }

    pub async fn stop(&mut self) -> Result {
        self.signal("INT").await
    }

    pub async fn signal(&mut self, signal: &str) -> Result {
        if let Some(child) = self.process.as_mut() {
            if child.try_wait()?.is_none() {
                let result = Command::new("kill")
                    .args([&format!("-{signal}"), &child.id().to_string()])
                    .status()?;
                assert!(result.success());
                let deadline = Instant::now() + Duration::from_secs(5);
                loop {
                    if let Some(exit) = child.try_wait()? {
                        if signal != "KILL" {
                            assert!(exit.success(), "Server exited with {exit}");
                        }
                        break;
                    }
                    if Instant::now() >= deadline {
                        child.kill()?;
                        child.wait()?;
                        return Err("Server did not stop within five seconds".into());
                    }
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
            }
        }
        self.process = None;
        Ok(())
    }

    pub fn api(&self, dashboard: bool) -> Result<Api> {
        let mut headers = HeaderMap::new();
        if dashboard {
            headers.insert("X-Partly-Request", "dashboard".parse()?);
        }
        let mut builder = Client::builder()
            .no_proxy()
            .cookie_store(true)
            .default_headers(headers)
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none());
        if let Some(ca) = &self.ca {
            builder = builder.add_root_certificate(reqwest::Certificate::from_pem(&fs::read(ca)?)?);
        }
        Ok(Api {
            client: builder.build()?,
            url: self.url.clone(),
        })
    }

    pub async fn setup(&self) -> Result<Api> {
        let api = self.api(true)?;
        api.post(
            "/api/setup",
            json!({"username":"admin","password":PASSWORD}),
            200,
        )
        .await?;
        Ok(api)
    }

    pub async fn login(&self, name: &str, password: &str) -> Result<Api> {
        let api = self.api(true)?;
        api.post(
            "/api/login",
            json!({"username":name,"password":password}),
            200,
        )
        .await?;
        Ok(api)
    }

    pub fn database(&self) -> Result<rusqlite::Connection> {
        let db = rusqlite::Connection::open(&self.db)?;
        // Corruption tests deliberately insert records the application would reject.
        db.pragma_update(None, "foreign_keys", false)?;
        Ok(db)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if let Some(mut child) = self.process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub async fn fixture() -> Result<(Server, Api)> {
    let mut server = Server::new()?;
    server.start().await?;
    let admin = server.setup().await?;
    Ok((server, admin))
}

pub struct Tls {
    pub _directory: TempDir,
    pub ca: PathBuf,
    pub cert: PathBuf,
    pub key: PathBuf,
    issuer: rcgen::Issuer<'static, rcgen::KeyPair>,
}

impl Tls {
    pub fn new(host: &str, expired: bool) -> Result<Self> {
        use rcgen::{BasicConstraints, CertificateParams, IsCa, Issuer, KeyPair, KeyUsagePurpose};
        let directory = tempfile::tempdir()?;
        let mut params = CertificateParams::new(vec!["Partly test CA".into()])?;
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, "Partly E2E CA");
        params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::DigitalSignature,
        ];
        let ca_key = KeyPair::generate()?;
        let ca_cert = params.self_signed(&ca_key)?;
        let issuer = Issuer::new(params, ca_key);
        let key_pair = KeyPair::generate()?;
        let mut params = CertificateParams::new(vec![host.into()])?;
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, host);
        params.use_authority_key_identifier_extension = true;
        params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
        if expired {
            params.not_before = time::OffsetDateTime::now_utc() - time::Duration::days(3);
            params.not_after = time::OffsetDateTime::now_utc() - time::Duration::days(1);
        }
        let cert = params.signed_by(&key_pair, &issuer)?;
        let ca = directory.path().join("ca.pem");
        let cert_path = directory.path().join("server.pem");
        let key = directory.path().join("server-key.pem");
        fs::write(&ca, ca_cert.pem())?;
        fs::write(&cert_path, format!("{}{}", cert.pem(), ca_cert.pem()))?;
        fs::write(&key, key_pair.serialize_pem())?;
        Ok(Self {
            _directory: directory,
            ca,
            cert: cert_path,
            key,
            issuer,
        })
    }

    pub fn rotate(&self, host: &str, expired: bool) -> Result {
        let key = rcgen::KeyPair::generate()?;
        let mut params = rcgen::CertificateParams::new(vec![host.into()])?;
        params
            .distinguished_name
            .push(rcgen::DnType::CommonName, host);
        params.use_authority_key_identifier_extension = true;
        params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ServerAuth];
        if expired {
            params.not_before = time::OffsetDateTime::now_utc() - time::Duration::days(3);
            params.not_after = time::OffsetDateTime::now_utc() - time::Duration::days(1);
        }
        let cert = params.signed_by(&key, &self.issuer)?;
        fs::write(
            &self.cert,
            format!("{}{}", cert.pem(), fs::read_to_string(&self.ca)?),
        )?;
        fs::write(&self.key, key.serialize_pem())?;
        Ok(())
    }

    pub fn configure(&self, server: &mut Server) {
        server.url = format!("https://127.0.0.1:{}", server.port);
        server.ca = Some(self.ca.clone());
        server.extra = vec![
            "--tls-cert".into(),
            self.cert.display().to_string(),
            "--tls-key".into(),
            self.key.display().to_string(),
        ];
    }
}

pub trait Stream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Stream for T {}

pub async fn connection(server: &Server) -> Result<Box<dyn Stream>> {
    let stream = tokio::net::TcpStream::connect(("127.0.0.1", server.port)).await?;
    if let Some(ca) = &server.ca {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let mut roots = rustls::RootCertStore::empty();
        for cert in rustls_pemfile::certs(&mut &fs::read(ca)?[..]) {
            roots.add(cert?)?;
        }
        let config = rustls::ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth();
        let connector = tokio_rustls::TlsConnector::from(std::sync::Arc::new(config));
        Ok(Box::new(
            connector.connect("127.0.0.1".try_into()?, stream).await?,
        ))
    } else {
        Ok(Box::new(stream))
    }
}
