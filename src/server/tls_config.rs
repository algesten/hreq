use crate::Error;
use rustls::internal::pemfile;
use std::fs::File;
use std::io::Cursor;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Configuration builder for `Server::listen_tls`.
#[derive(Debug, Clone)]
pub struct TlsConfig {
    key: Option<MemOrFile>,
    cert: Option<MemOrFile>,
}

#[derive(Debug, Clone)]
enum MemOrFile {
    Mem(Vec<u8>),
    File(PathBuf),
}

impl MemOrFile {
    fn into_bytes(self) -> Result<Vec<u8>, Error> {
        match self {
            MemOrFile::Mem(v) => Ok(v),
            MemOrFile::File(p) => {
                let mut f = File::open(&p)?;
                let mut buf = Vec::new();
                f.read_to_end(&mut buf)?;
                Ok(buf)
            }
        }
    }
}

impl TlsConfig {
    /// Create a new TLS configuration builder.
    pub fn new() -> Self {
        TlsConfig {
            key: None,
            cert: None,
        }
    }

    /// Configure in memory contents for private key.
    ///
    /// The contents should be a PEM encoded PKCS8 private key. The contents
    /// can be a mix of certificates and keys. The first found key is used.
    pub fn key(mut self, key: impl AsRef<[u8]>) -> Self {
        self.key = Some(MemOrFile::Mem(key.as_ref().to_vec()));
        self
    }

    /// Configure in memory contents for certificate.
    ///
    /// The contents should be a PEM encoded chain of certificates. The private
    /// key can be in the same contents and is ignored.
    ///
    /// Note that the end-entity certificate must have the [Subject Alternative Name]
    /// extension to describe the valid DNS name. The `commonName` field is disregarded.
    ///
    /// [Subject Alternative Name]: https://tools.ietf.org/html/rfc6125#section-4.1
    pub fn cert(mut self, cert: impl AsRef<[u8]>) -> Self {
        self.cert = Some(MemOrFile::Mem(cert.as_ref().to_vec()));
        self
    }

    /// Configure private key as a path to a file.
    ///
    /// The contents should be a PEM encoded PKCS8 private key. The contents
    /// can be a mix of certificates and keys. The first found key is used.
    pub fn key_path(mut self, path: impl AsRef<Path>) -> Self {
        self.key = Some(MemOrFile::File(path.as_ref().to_path_buf()));
        self
    }

    /// Configure certificate as a path to a file.
    ///
    /// The contents should be a PEM encoded chain of certificates. The private
    /// key can be in the same contents and is ignored.
    ///
    /// Note that the end-entity certificate must have the [Subject Alternative Name]
    /// extension to describe the valid DNS name. The `commonName` field is disregarded.
    ///
    /// [Subject Alternative Name]: https://tools.ietf.org/html/rfc6125#section-4.1
    pub fn cert_path(mut self, path: impl AsRef<Path>) -> Self {
        self.cert = Some(MemOrFile::File(path.as_ref().to_path_buf()));
        self
    }

    pub(crate) fn into_rustls_config(self) -> Result<rustls::ServerConfig, Error> {
        let key_buf = self
            .key
            .ok_or_else(|| Error::User("TlsConfig missing private key".into()))?
            .into_bytes()?;

        let cert_buf = self
            .cert
            .ok_or_else(|| Error::User("TlsConfig missing certificate".into()))?
            .into_bytes()?;

        let mut key_cur = Cursor::new(key_buf);
        let mut keys = pemfile::pkcs8_private_keys(&mut key_cur)
            .map_err(|_| Error::User("TlsConfig failed to extract private key".into()))?;
        let key = keys
            .pop()
            .ok_or_else(|| Error::User("Found no private key in TlsConfig".into()))?;

        let mut cert_cur = Cursor::new(cert_buf);
        let certs = pemfile::certs(&mut cert_cur)
            .map_err(|_| Error::User("TlsConfig failed to extract certificates".into()))?;
        if certs.is_empty() {
            return Err(Error::User("No certificates in TlsConfig".into()));
        }

        let mut config = rustls::ServerConfig::new(rustls::NoClientAuth::new());

        config.set_single_cert(certs, key)?;

        Ok(config)
    }
}
