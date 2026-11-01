// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::{Api, PASSWORD, Result, binary};
use reqwest::{Client, Method, header::HeaderMap};
use serde_json::json;
use std::{
    fs::{self, File},
    net::TcpListener,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
use tempfile::TempDir;

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
