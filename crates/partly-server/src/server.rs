// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::{
    dashboard, logging,
    state::App,
    store::Store,
    transport::{Transport, security},
};
use axum::{extract::DefaultBodyLimit, middleware};
use std::{
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("Could not register SIGTERM handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

pub(crate) async fn serve(
    host: IpAddr,
    port: u16,
    db: PathBuf,
    tls_cert: Option<PathBuf>,
    tls_key: Option<PathBuf>,
    hostname: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let tls_config = if let (Some(cert), Some(key)) = (tls_cert, tls_key) {
        rustls::crypto::ring::default_provider()
            .install_default()
            .map_err(|_| "Could not initialize the TLS crypto provider")?;
        Some(axum_server::tls_rustls::RustlsConfig::from_pem_file(cert, key).await?)
    } else {
        None
    };
    let transport = Transport {
        tls: tls_config.is_some(),
        hostname,
    };
    let scheme = transport.scheme();
    let store = Store::open(db).map_err(|e| e.1)?;
    let setup_required = !store.ready();
    if !store.ready() && !host.is_loopback() {
        return Err("Complete setup locally first: run with --host 127.0.0.1, create the administrator, then restart with the remote bind address".into());
    }
    let app = dashboard::router()
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(middleware::from_fn_with_state(transport, security))
        .layer(middleware::from_fn(logging::request))
        .with_state(App::new(store));
    let listener = tokio::net::TcpListener::bind((host, port)).await?;
    println!("Partly listening at {scheme}://{}", listener.local_addr()?);
    tracing::info!(target: "partly", version = env!("CARGO_PKG_VERSION"), scheme, address = %listener.local_addr()?, setup_required, "Server ready");
    let handle = axum_server::Handle::new();
    let shutdown = handle.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        tracing::info!(target: "partly", "Shutdown requested");
        shutdown.graceful_shutdown(Some(std::time::Duration::from_secs(3)));
    });
    let listener = listener.into_std()?;
    let service = app.into_make_service_with_connect_info::<SocketAddr>();
    if let Some(tls) = tls_config {
        axum_server::from_tcp_rustls(listener, tls)?
            .handle(handle)
            .serve(service)
            .await?;
    } else {
        axum_server::from_tcp(listener)?
            .handle(handle)
            .serve(service)
            .await?;
    }
    tracing::info!(target: "partly", "Server stopped");
    Ok(())
}
