//! Against a real server (NFS_SERVER=host:port); skipped without it.

use nfs_rpc::{Auth, Connection, Error};
use std::time::Duration;
use tokio::net::TcpStream;

fn server() -> Option<String> {
    std::env::var("NFS_SERVER").ok()
}

async fn connect(server: &str) -> nfs_rpc::Stream {
    Box::new(TcpStream::connect(server).await.expect("connect"))
}

#[tokio::test]
async fn null_calls_in_flight_at_once() {
    let Some(server) = server() else { return };
    let connection = Connection::new(connect(&server).await, Duration::from_secs(10));
    let mut calls = tokio::task::JoinSet::new();
    for _ in 0..64 {
        let connection = connection.clone();
        calls.spawn(
            async move { connection.call(0, &Auth::None, |_| {}).await.map(|d| d.remaining()) },
        );
    }
    while let Some(result) = calls.join_next().await {
        assert_eq!(result.unwrap(), Ok(0));
    }
}

#[tokio::test]
async fn starttls_refused_by_a_server_without_tls() {
    let Some(server) = server() else { return };
    if std::env::var("NFS_SERVER_TLS").is_ok() {
        return;
    }
    let config = rustls::ClientConfig::builder()
        .with_root_certificates(rustls::RootCertStore::empty())
        .with_no_client_auth();
    let name = rustls::pki_types::ServerName::try_from("localhost").unwrap();
    let result = nfs_rpc::starttls(connect(&server).await, config.into(), name).await;
    assert!(matches!(result, Err(Error::Tls(_))), "{:?}", result.err());
}
