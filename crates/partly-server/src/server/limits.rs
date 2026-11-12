// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use axum::{
    Json,
    body::{Body, Bytes},
    extract::{FromRequest, Request, State},
    http::{StatusCode, Version, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use axum_server::accept::Accept;
use std::{
    future::Future,
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::TcpStream,
    sync::{OwnedSemaphorePermit, Semaphore},
};

pub(super) const MAX_CONNECTIONS: usize = 64;
pub(super) const MAX_REQUESTS: usize = 64;
pub(super) const MAX_BODY_BYTES: usize = 16 * 1024;
const BODY_DEADLINE: Duration = Duration::from_secs(10);

pub(super) async fn request(
    State(slots): State<Arc<Semaphore>>,
    request: Request,
    next: Next,
) -> Response {
    let Ok(_permit) = slots.try_acquire() else {
        let mut response = rejected(StatusCode::SERVICE_UNAVAILABLE, request.version());
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, "1".parse().unwrap());
        return response;
    };
    let (parts, body) = request.into_parts();
    let read = Bytes::from_request(Request::from_parts(parts.clone(), body), &());
    let result = tokio::time::timeout(BODY_DEADLINE, read).await;
    let status = match result {
        Ok(Ok(bytes)) => {
            return next
                .run(Request::from_parts(parts, Body::from(bytes)))
                .await;
        }
        Ok(Err(error)) => error.status(),
        Err(_) => StatusCode::REQUEST_TIMEOUT,
    };
    rejected(status, parts.version)
}

fn rejected(status: StatusCode, version: Version) -> Response {
    let mut response = (
        status,
        Json(crate::api::ErrorBody {
            error: status
                .canonical_reason()
                .unwrap_or("Request rejected")
                .into(),
            code: None,
        }),
    )
        .into_response();
    if matches!(version, Version::HTTP_10 | Version::HTTP_11) {
        response
            .headers_mut()
            .insert(header::CONNECTION, "close".parse().unwrap());
    }
    response
}

#[derive(Clone)]
pub(super) struct LimitedAcceptor<A> {
    inner: A,
    slots: Arc<Semaphore>,
}

impl<A> LimitedAcceptor<A> {
    pub(super) fn new(inner: A) -> Self {
        Self {
            inner,
            slots: Arc::new(Semaphore::new(MAX_CONNECTIONS)),
        }
    }
}

impl<A, S> Accept<TcpStream, S> for LimitedAcceptor<A>
where
    A: Accept<TcpStream, S> + Clone + Send + 'static,
    A::Future: Send,
    S: Send + 'static,
{
    type Stream = Connection<A::Stream>;
    type Service = A::Service;
    type Future = Pin<Box<dyn Future<Output = io::Result<(Self::Stream, Self::Service)>> + Send>>;

    fn accept(&self, stream: TcpStream, service: S) -> Self::Future {
        let permit = self.slots.clone().try_acquire_owned();
        let inner = self.inner.clone();
        Box::pin(async move {
            // Reject immediately, before TLS work, rather than queueing open sockets.
            let permit = permit.map_err(|_| io::Error::other("Connection limit reached"))?;
            let (stream, service) = inner.accept(stream, service).await?;
            Ok((
                Connection {
                    stream,
                    _permit: permit,
                },
                service,
            ))
        })
    }
}

pub(super) struct Connection<T> {
    stream: T,
    _permit: OwnedSemaphorePermit,
}

impl<T: AsyncRead + Unpin> AsyncRead for Connection<T> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_read(cx, buf)
    }
}

impl<T: AsyncWrite + Unpin> AsyncWrite for Connection<T> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().stream).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().stream).poll_shutdown(cx)
    }
}

#[cfg(test)]
mod tests;
