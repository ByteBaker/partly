// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::*;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Clone)]
pub struct Reply {
    pub code: u16,
    pub body: Vec<u8>,
    pub headers: Vec<(String, String)>,
    pub delay: Duration,
}
impl Reply {
    pub fn policy(code: u16, body: impl Into<Vec<u8>>, etag: Option<&str>) -> Self {
        Self {
            code,
            body: body.into(),
            headers: etag
                .map(|value| vec![("ETag".into(), value.into())])
                .unwrap_or_default(),
            delay: Duration::ZERO,
        }
    }
}

type Request = (String, Option<String>);

pub struct HttpStub {
    pub url: String,
    pub reply: Arc<Mutex<Reply>>,
    pub requests: Arc<Mutex<Vec<Request>>>,
    task: tokio::task::JoinHandle<()>,
}

impl HttpStub {
    pub async fn new(reply: Reply) -> Result<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}", listener.local_addr()?);
        let reply = Arc::new(Mutex::new(reply));
        let requests = Arc::new(Mutex::new(vec![]));
        let response = reply.clone();
        let seen = requests.clone();
        let task = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let Ok((mut stream, _)) = accepted else { break };
                        let response = response.clone();
                        let seen = seen.clone();
                        connections.spawn(async move {
                            let mut bytes = vec![];
                            while bytes.len() < 16384 && !bytes.ends_with(b"\r\n\r\n") {
                                match tokio::time::timeout(Duration::from_secs(5), stream.read_u8()).await {
                                    Ok(Ok(byte)) => bytes.push(byte),
                                    _ => return,
                                }
                            }
                            let request = String::from_utf8_lossy(&bytes);
                            let path = request.lines().next().unwrap_or("")
                                .split_whitespace().nth(1).unwrap_or("").to_owned();
                            let etag = request.lines().find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("if-none-match").then(|| value.trim().to_owned())
                            });
                            seen.lock().unwrap().push((path, etag));
                            let reply = response.lock().unwrap().clone();
                            let mut headers = format!(
                                "HTTP/1.1 {} Test\r\nConnection: close\r\nContent-Length: {}\r\n",
                                reply.code, reply.body.len()
                            );
                            for (name, value) in reply.headers {
                                headers.push_str(&format!("{name}: {value}\r\n"));
                            }
                            headers.push_str("\r\n");
                            if stream.write_all(headers.as_bytes()).await.is_err() { return; }
                            tokio::time::sleep(reply.delay).await;
                            let _ = stream.write_all(&reply.body).await;
                        });
                    }
                    Some(_) = connections.join_next(), if !connections.is_empty() => {}
                }
            }
        });
        Ok(Self {
            url,
            reply,
            requests,
            task,
        })
    }
    pub fn respond(&self, reply: Reply) {
        *self.reply.lock().unwrap() = reply;
    }
}
impl Drop for HttpStub {
    fn drop(&mut self) {
        self.task.abort();
    }
}
