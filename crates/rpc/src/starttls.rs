//! RPC-with-TLS (RFC 9289): a NULL call with AUTH_TLS asks the server; it answers "STARTTLS" in
//! the verifier, and TLS starts on the same connection. `config` carries ALPN "sunrpc".

use crate::incoming;
use crate::message::{self, Auth};
use crate::{Error, Stream, record};
use rustls::ClientConfig;
use rustls::pki_types::ServerName;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio_rustls::TlsConnector;

pub async fn starttls(
    mut stream: Stream,
    config: Arc<ClientConfig>,
    name: ServerName<'static>,
) -> Result<Stream, Error> {
    let io = |e: std::io::Error| Error::Disconnected(e.to_string());
    stream.write_all(&message::call(0, 0, &Auth::Tls, |_| {})).await.map_err(io)?;
    let reply = incoming::parse(record::read(&mut stream).await.map_err(io)?)?;
    let offered = matches!(reply, incoming::Incoming::Reply(r)
        if r.result.is_ok() && &r.verifier[..] == b"STARTTLS");
    if !offered {
        return Err(Error::Tls("the server does not offer RPC-with-TLS".into()));
    }
    let tls = TlsConnector::from(config).connect(name, stream).await;
    let tls = tls.map_err(|e| Error::Tls(e.to_string()))?;
    // RFC 9289 section 5.1.1: TLS 1.3 or later, whatever else the configuration would allow.
    if tls.get_ref().1.protocol_version() != Some(rustls::ProtocolVersion::TLSv1_3) {
        return Err(Error::Tls("the server does not speak TLS 1.3".into()));
    }
    Ok(Box::new(tls))
}
