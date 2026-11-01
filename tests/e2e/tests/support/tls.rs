// Copyright 2026 ByteBaker
// SPDX-License-Identifier: Elastic-2.0

use super::{Result, Server};
use rustls::pki_types::{CertificateDer, pem::PemObject};
use std::{fs, path::PathBuf};
use tempfile::TempDir;

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
        for cert in CertificateDer::pem_slice_iter(&fs::read(ca)?) {
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
