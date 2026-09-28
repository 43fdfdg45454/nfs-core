//! RPC-with-TLS speaks TLS 1.3 only: a server offering TLS 1.2 is refused even by a client
//! configuration that would allow it. Against a server in the test itself.

use nfs_rpc::Error;
use rustls::pki_types::{PrivateKeyDer, ServerName};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// A server that answers the STARTTLS probe, then speaks TLS in only the versions given.
async fn server(
    versions: &'static [&'static rustls::SupportedProtocolVersion],
) -> (u16, rustls::pki_types::CertificateDer<'static>) {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let der = cert.cert.der().clone();
    let key = PrivateKeyDer::try_from(cert.key_pair.serialize_der()).unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(versions)
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![der.clone()], key)
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut tcp, _) = listener.accept().await.unwrap();
        let mut probe = [0u8; 4];
        tcp.read_exact(&mut probe).await.unwrap();
        let mut rest = vec![0; (u32::from_be_bytes(probe) & 0x7fff_ffff) as usize];
        tcp.read_exact(&mut rest).await.unwrap();
        // xid 0, a reply, accepted, verifier AUTH_TLS (7) "STARTTLS", success.
        let mut reply = Vec::new();
        for word in [0u32, 1, 0, 7, 8] {
            reply.extend(word.to_be_bytes());
        }
        reply.extend(b"STARTTLS");
        reply.extend(0u32.to_be_bytes());
        let mark = 0x8000_0000 | reply.len() as u32;
        tcp.write_all(&[&mark.to_be_bytes()[..], &reply].concat()).await.unwrap();
        _ = tokio_rustls::TlsAcceptor::from(Arc::new(config)).accept(tcp).await;
    });
    (port, der)
}

async fn starttls(
    versions: &'static [&'static rustls::SupportedProtocolVersion],
) -> Result<(), Error> {
    let (port, cert) = server(versions).await;
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert).unwrap();
    // A client that would take TLS 1.2 too: the refusal must not rest on its configuration.
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_protocol_versions(rustls::ALL_VERSIONS)
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let stream: nfs_rpc::Stream = Box::new(TcpStream::connect(("127.0.0.1", port)).await.unwrap());
    let name = ServerName::try_from("localhost").unwrap();
    nfs_rpc::starttls(stream, Arc::new(config), name).await.map(drop)
}

static TLS12: &[&rustls::SupportedProtocolVersion] = &[&rustls::version::TLS12];
static TLS13: &[&rustls::SupportedProtocolVersion] = &[&rustls::version::TLS13];

#[tokio::test]
async fn a_tls_1_2_server_is_refused() {
    let result = starttls(TLS12).await;
    assert!(matches!(&result, Err(Error::Tls(why)) if why.contains("1.3")), "{result:?}");
}

#[tokio::test]
async fn a_tls_1_3_server_is_taken() {
    starttls(TLS13).await.expect("TLS 1.3");
}
