//! rustls configurations from PEM files. The client authenticates with a certificate of the CA
//! the gateway trusts; the gateway can also run without asking for one.

use crate::Error;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::server::WebPkiClientVerifier;
use rustls::{ClientConfig, RootCertStore, ServerConfig};
use std::path::Path;
use std::sync::Arc;

const ALPN: &[u8] = b"h3";

pub fn certs(path: &Path) -> Result<Vec<CertificateDer<'static>>, Error> {
    Ok(CertificateDer::pem_file_iter(path)?.collect::<Result<_, _>>()?)
}

pub fn key(path: &Path) -> Result<PrivateKeyDer<'static>, Error> {
    Ok(PrivateKeyDer::from_pem_file(path)?)
}

fn roots(ca: &Path) -> Result<Arc<RootCertStore>, Error> {
    let mut roots = RootCertStore::empty();
    for cert in certs(ca)? {
        roots.add(cert)?;
    }
    Ok(Arc::new(roots))
}

/// The gateway's side. `client_ca`: the CA client certificates must chain to, or `None` to
/// accept clients without one.
pub fn server(
    cert: &Path,
    key_path: &Path,
    client_ca: Option<&Path>,
) -> Result<ServerConfig, Error> {
    let builder = ServerConfig::builder();
    let builder = match client_ca {
        Some(ca) => {
            builder.with_client_cert_verifier(WebPkiClientVerifier::builder(roots(ca)?).build()?)
        }
        None => builder.with_no_client_auth(),
    };
    let mut config = builder.with_single_cert(certs(cert)?, key(key_path)?)?;
    config.alpn_protocols = vec![ALPN.into()];
    Ok(config)
}

/// The client's side: trusts `ca` only, and presents `identity` (certificate, key) if given.
pub fn client(ca: &Path, identity: Option<(&Path, &Path)>) -> Result<ClientConfig, Error> {
    let builder = ClientConfig::builder().with_root_certificates(roots(ca)?);
    let mut config = match identity {
        Some((cert, key_path)) => builder.with_client_auth_cert(certs(cert)?, key(key_path)?)?,
        None => builder.with_no_client_auth(),
    };
    config.alpn_protocols = vec![ALPN.into()];
    Ok(config)
}
