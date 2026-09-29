//! The client side of the tunnel: one QUIC connection to the gateway, opened on first use and
//! again after it closes (a new address means a new connection: the gateway does not follow
//! migrations), and a CONNECT stream for each TCP connection the NFS client would open. It is
//! opened as a TCP connection is: the name looked up each time (the network may have changed
//! what it points to), and each of its addresses tried in turn.

use crate::Error;
use crate::pump::{self, Io};
use bytes::Bytes;
use h3::client::SendRequest;
use http::{Method, Request, StatusCode};
use std::sync::Arc;
use tokio::sync::Mutex;

type Sender = SendRequest<h3_quinn::OpenStreams, Bytes>;

/// How long the gateway has to answer a CONNECT (with the QUIC handshake, if there is none).
const ANSWER: std::time::Duration = std::time::Duration::from_secs(4);
type Stream = h3::client::RequestStream<h3_quinn::BidiStream<Bytes>, Bytes>;

pub struct Tunnel {
    pub endpoint: quinn::Endpoint,
    /// `host:port` of the gateway, as for TCP: a name or an address (IPv6 in brackets).
    pub gateway: String,
    pub server_name: String,
    pub authority: String,
    /// An extra request header, `name:value`, to show the gateway ignores headers.
    pub header: Option<String>,
    /// The current connection's id, request sender and connection; cleared when it closes.
    pub send_request: Arc<Mutex<Option<(usize, Sender, quinn::Connection)>>>,
}

/// The QUIC path to the gateway: round trip, congestion window, packets sent and lost.
#[derive(Debug, Clone, Copy)]
pub struct Path {
    pub rtt: std::time::Duration,
    pub cwnd: u64,
    pub sent: u64,
    pub lost: u64,
}

impl Tunnel {
    /// The current connection's path, if connected (for diagnostics).
    pub fn path(&self) -> Option<Path> {
        let slot = self.send_request.try_lock().ok()?;
        let path = slot.as_ref()?.2.stats().path;
        Some(Path {
            rtt: path.rtt,
            cwnd: path.cwnd,
            sent: path.sent_packets,
            lost: path.lost_packets,
        })
    }

    async fn sender(&self) -> Result<Sender, Error> {
        let mut slot = self.send_request.lock().await;
        if let Some((_, sender, _)) = slot.as_ref() {
            return Ok(sender.clone());
        }
        let connection = self.connect().await?;
        let id = connection.stable_id();
        let quic = connection.clone();
        let (mut driver, sender) = h3::client::new(h3_quinn::Connection::new(connection)).await?;
        *slot = Some((id, sender.clone(), quic));
        let shared = self.send_request.clone();
        tokio::spawn(async move {
            let error = std::future::poll_fn(|cx| driver.poll_close(cx)).await;
            eprintln!("connection to the gateway closed: {error}");
            let mut slot = shared.lock().await;
            if slot.as_ref().is_some_and(|(current, _, _)| *current == id) {
                slot.take();
            }
        });
        Ok(sender)
    }

    async fn connect(&self) -> Result<quinn::Connection, Error> {
        let mut failed: Option<Error> = None;
        for address in tokio::net::lookup_host(self.gateway.as_str()).await? {
            match self.endpoint.connect(address, &self.server_name) {
                Ok(connecting) => match connecting.await {
                    Ok(connection) => return Ok(connection),
                    Err(error) => failed = Some(error.into()),
                },
                Err(error) => failed = Some(error.into()),
            }
        }
        Err(failed.unwrap_or_else(|| format!("{} has no address", self.gateway).into()))
    }

    /// The network changed (another interface or address): the connection on the old path is
    /// closed at once, and the next stream opens a new one instead of waiting for it to time out.
    pub async fn reset(&self) {
        if let Some((_, _, connection)) = self.send_request.lock().await.take() {
            connection.close(0u32.into(), b"network changed");
        }
    }

    /// A CONNECT stream the gateway accepted, within [`ANSWER`]: otherwise the QUIC connection is
    /// taken for dead (its path went: another address, a network down) and replaced, instead of
    /// waiting for its idle timeout.
    async fn open(&self) -> Result<Stream, Error> {
        match tokio::time::timeout(ANSWER, self.try_open()).await {
            Ok(stream) => stream,
            Err(_) => {
                self.reset().await;
                Err(format!("the gateway did not answer in {ANSWER:?}").into())
            }
        }
    }

    async fn try_open(&self) -> Result<Stream, Error> {
        let mut request = Request::builder().method(Method::CONNECT).uri(self.authority.as_str());
        if let Some((name, value)) = self.header.as_deref().and_then(|h| h.split_once(':')) {
            request = request.header(name, value);
        }
        let mut stream = self.sender().await?.send_request(request.body(())?).await?;
        let status = stream.recv_response().await?.status();
        if status != StatusCode::OK {
            return Err(format!("gateway answered {status}").into());
        }
        Ok(stream)
    }

    /// Carries `io` (a TCP connection) as a CONNECT stream; returns the bytes sent up and down.
    pub async fn carry(&self, io: impl Io) -> Result<(u64, u64), Error> {
        let (send, recv) = self.open().await?.split();
        Ok(pump::pump(send, recv, io).await?)
    }

    /// A CONNECT stream as a byte stream, for a client in this process.
    pub async fn stream(&self) -> Result<tokio::io::DuplexStream, Error> {
        let (send, recv) = self.open().await?.split();
        let (near, far) = tokio::io::duplex(256 << 10);
        tokio::spawn(pump::pump(send, recv, far));
        Ok(near)
    }
}
