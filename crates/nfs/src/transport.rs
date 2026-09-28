//! How the client reaches the server: TCP straight to nfsd, or HTTP/3 CONNECT streams through the
//! gateway; either way with the TLS the export asks for, end to end.

use crate::config::Config;
use crate::error::{Error, Result};
use nfs_rpc::{Connection, Stream};
use nfs_tunnel::client::Tunnel;
use rustls::ClientConfig;
use rustls::pki_types::ServerName;
use std::sync::Arc;
use tokio::net::TcpStream;

#[derive(Clone)]
pub enum Transport {
    /// `host:port` of nfsd (or of a NAS).
    Tcp(String),
    /// Each connection is a CONNECT stream of this tunnel's one QUIC connection.
    Quic(Arc<Tunnel>),
}

#[derive(Clone)]
pub enum Security {
    None,
    /// RPC-with-TLS: `config` trusts the server's CA and may carry the client's certificate.
    Tls {
        config: Arc<ClientConfig>,
        name: ServerName<'static>,
    },
}

impl Security {
    /// A rustls configuration for RPC-with-TLS: ALPN "sunrpc" (RFC 9289 section 5.2).
    pub fn tls(mut config: ClientConfig, name: &str) -> Result<Self> {
        config.alpn_protocols = vec![b"sunrpc".to_vec()];
        let name =
            ServerName::try_from(name.to_owned()).map_err(|e| Error::Other(e.to_string()))?;
        Ok(Self::Tls { config: Arc::new(config), name })
    }
}

pub async fn connect(config: &Config) -> Result<Connection> {
    let lost = |e: String| Error::Rpc(nfs_rpc::Error::Disconnected(e));
    let reach = async {
        Ok::<Stream, String>(match &config.transport {
            Transport::Tcp(server) => {
                let tcp = TcpStream::connect(server).await.map_err(|e| e.to_string())?;
                tcp.set_nodelay(true).map_err(|e| e.to_string())?;
                Box::new(tcp)
            }
            Transport::Quic(tunnel) => Box::new(tunnel.stream().await.map_err(|e| e.to_string())?),
        })
    };
    // Over QUIC, the tunnel also replaces a connection that stopped answering (nfs-tunnel).
    let stream = tokio::time::timeout(config.reach_timeout, reach)
        .await
        .map_err(|_| lost(format!("no answer in {:?}", config.reach_timeout)))?
        .map_err(lost)?;
    let (security, idle_timeout) = (&config.security, config.idle_timeout);
    let stream = match security {
        Security::None => stream,
        Security::Tls { config, name } => {
            nfs_rpc::starttls(stream, config.clone(), name.clone()).await?
        }
    };
    Ok(Connection::new(stream, idle_timeout))
}
