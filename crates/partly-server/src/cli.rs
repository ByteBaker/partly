// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use crate::{api, auth, logging, store::Store, transport::parse_hostname};
use clap::{Parser, Subcommand};
use std::{
    io::{IsTerminal, Read},
    net::IpAddr,
    path::PathBuf,
};

#[derive(Parser)]
#[command(name = "partly", version, about = "Local feature flag control plane")]
struct Cli {
    /// Operational log level: off, error, warn, info, debug, or trace. Logs go to stderr.
    #[arg(long, global = true, default_value = "info")]
    log_level: tracing::level_filters::LevelFilter,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print the generated OpenAPI contract without opening a database or listener.
    Openapi {
        /// Write JSON to a file instead of standard output.
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Reset the local administrator password from one line on standard input. Stop the server first.
    ResetPassword {
        #[arg(long, default_value = "./partly.db")]
        db: PathBuf,
    },
    Serve {
        #[arg(long, default_value = "127.0.0.1")]
        host: IpAddr,
        #[arg(long, default_value_t = 8080)]
        port: u16,
        #[arg(long, default_value = "./partly.db")]
        db: PathBuf,
        /// PEM certificate chain for native HTTPS. Requires --tls-key.
        #[arg(long, requires = "tls_key")]
        tls_cert: Option<PathBuf>,
        /// PEM private key matching --tls-cert.
        #[arg(long, requires = "tls_cert")]
        tls_key: Option<PathBuf>,
        /// Additional DNS hostname to accept, without scheme or port.
        #[arg(long, value_parser = parse_hostname)]
        hostname: Option<String>,
    },
}
pub(crate) async fn run() {
    let cli = Cli::parse();
    logging::init(cli.log_level);
    let reset = matches!(cli.command, Command::ResetPassword { .. });
    let export = matches!(cli.command, Command::Openapi { .. });
    let result = match cli.command {
        Command::Openapi { output } => export_openapi(output),
        Command::ResetPassword { db } => reset_password(db),
        Command::Serve {
            host,
            port,
            db,
            tls_cert,
            tls_key,
            hostname,
        } => crate::server::serve(host, port, db, tls_cert, tls_key, hostname).await,
    };
    if let Err(error) = result {
        if reset {
            tracing::error!(target: "partly", error = ?error.to_string(), "Password reset failed");
        } else if export {
            tracing::error!(target: "partly", error = ?error.to_string(), "OpenAPI export failed");
        } else {
            tracing::error!(target: "partly", error = ?error.to_string(), "Partly could not start or serve requests");
        }
        std::process::exit(1);
    }
}

fn export_openapi(output: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let json = api::router().1.to_json()? + "\n";
    match output {
        Some(path) => std::fs::write(path, json)?,
        None => print!("{json}"),
    }
    Ok(())
}

fn reset_password(db: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let mut store = Store::open(db).map_err(|e| e.1)?;
    if !store.ready() {
        return Err("Database does not exist; complete setup first".into());
    }
    if std::io::stdin().is_terminal() {
        return Err("Read the password privately and pipe one line to reset-password; terminal input would expose it".into());
    }
    let mut password = String::new();
    std::io::stdin().take(1027).read_to_string(&mut password)?;
    if password.ends_with('\n') {
        password.pop();
        if password.ends_with('\r') {
            password.pop();
        }
    }
    if password.contains(['\r', '\n']) {
        return Err("Supply exactly one password line on standard input".into());
    }
    let hash = auth::hash_password(&password).map_err(|e| e.1)?;
    store.reset_password(&hash).map_err(|e| e.1)?;
    tracing::info!(target: "partly", "Administrator password reset");
    println!("Administrator password reset. Start the server and sign in with the new password.");
    Ok(())
}
