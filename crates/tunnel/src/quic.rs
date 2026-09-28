//! QUIC endpoints. One connection per server carries every tunnel, so its settings are what
//! decides the throughput over a lossy link.

use crate::Error;
use crate::congestion::Congestion;
use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use quinn::{Endpoint, EndpointConfig, TokioRuntime, TransportConfig, VarInt};
use std::net::{SocketAddr, UdpSocket};
use std::sync::Arc;
use std::time::Duration;

/// Flow-control windows, many times the VPN link's bandwidth-delay product (1.25 MB): a stream
/// held by a loss (it delivers in order) must never stop the sender.
const STREAM_WINDOW: u32 = 16 << 20;
const CONNECTION_WINDOW: u32 = 64 << 20;
/// Socket buffers asked for; the kernel caps them at net.core.{r,w}mem_max.
const SOCKET_BUFFER: usize = 8 << 20;

fn transport(congestion: Congestion) -> TransportConfig {
    let mut config = TransportConfig::default();
    config
        .stream_receive_window(VarInt::from_u32(STREAM_WINDOW))
        .receive_window(VarInt::from_u32(CONNECTION_WINDOW))
        .send_window(CONNECTION_WINDOW.into())
        .max_concurrent_bidi_streams(VarInt::from_u32(256))
        // A ping every 25 s keeps NAT mappings (30 s at the least) without keeping a phone's radio
        // awake (it rests some 10 s after the last packet). A dead path under waiting calls is
        // noticed by the RPC idle timeout and the reach timeout of a new stream, not by this.
        .keep_alive_interval(Some(Duration::from_secs(25)))
        .max_idle_timeout(Some(VarInt::from_u32(60_000).into()));
    config.congestion_controller_factory(congestion.factory());
    config
}

fn socket(addr: SocketAddr) -> Result<UdpSocket, Error> {
    let socket = UdpSocket::bind(addr)?;
    let raw = socket2::SockRef::from(&socket);
    raw.set_recv_buffer_size(SOCKET_BUFFER)?;
    raw.set_send_buffer_size(SOCKET_BUFFER)?;
    Ok(socket)
}

/// The gateway's endpoint. Migration is off: the client's address is what nfsd sees, so a
/// client that changes address comes back with a new connection instead.
pub fn server(
    addr: SocketAddr,
    tls: rustls::ServerConfig,
    congestion: Congestion,
) -> Result<Endpoint, Error> {
    let mut config = quinn::ServerConfig::with_crypto(Arc::new(QuicServerConfig::try_from(tls)?));
    config.transport_config(Arc::new(transport(congestion))).migration(false);
    let runtime = Arc::new(TokioRuntime);
    Ok(Endpoint::new(EndpointConfig::default(), Some(config), socket(addr)?, runtime)?)
}

pub fn client(tls: rustls::ClientConfig, congestion: Congestion) -> Result<Endpoint, Error> {
    let mut config = quinn::ClientConfig::new(Arc::new(QuicClientConfig::try_from(tls)?));
    config.transport_config(Arc::new(transport(congestion)));
    let socket = socket(SocketAddr::from(([0, 0, 0, 0], 0)))?;
    let mut endpoint =
        Endpoint::new(EndpointConfig::default(), None, socket, Arc::new(TokioRuntime))?;
    endpoint.set_default_client_config(config);
    Ok(endpoint)
}
