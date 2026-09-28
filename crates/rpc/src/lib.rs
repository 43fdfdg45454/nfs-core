//! ONC RPC (RFC 5531) for NFSv4 over any byte stream (TCP, a TLS session, an HTTP/3 CONNECT
//! stream): many calls in flight on one connection, matched to their replies by xid.

mod connection;
#[cfg(feature = "fuzzing")]
#[doc(hidden)]
pub mod fuzzing;
mod incoming;
mod message;
mod progress;
mod receive;
mod record;
mod starttls;
mod traffic;

pub use connection::Connection;
pub use message::{Auth, SysCred};
pub use nfs_xdr::Decoder;
pub use starttls::starttls;
use tokio::io::{AsyncRead, AsyncWrite};
pub use traffic::traffic;

/// The NFS program and the one version spoken.
pub const NFS_PROGRAM: u32 = 100_003;
pub const NFS_V4: u32 = 4;

/// Serves the calls the server makes on a connection (NFSv4.1 callbacks): the results of a
/// procedure, or `None` for a program or version it does not serve.
pub trait Handler: Send + Sync + 'static {
    fn call(
        &self,
        program: u32,
        version: u32,
        procedure: u32,
        args: Decoder,
    ) -> Option<bytes::Bytes>;
}

/// A byte stream an RPC connection runs over.
pub trait Io: AsyncRead + AsyncWrite + Send + Unpin + 'static {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin + 'static> Io for T {}
pub type Stream = Box<dyn Io>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The connection is gone; the call may be sent again on another one.
    Disconnected(String),
    /// Calls were waiting and nothing arrived for the idle timeout: the connection was closed.
    Stalled,
    /// The server refused the call at the RPC level (authentication, version, arguments).
    Rejected(String),
    /// RPC-with-TLS could not be set up.
    Tls(String),
    /// A reply that cannot be decoded.
    Garbage(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disconnected(why) => write!(f, "connection lost: {why}"),
            Self::Stalled => f.write_str("the server stopped answering"),
            Self::Rejected(why) => write!(f, "RPC call refused: {why}"),
            Self::Tls(why) => write!(f, "RPC-with-TLS: {why}"),
            Self::Garbage(why) => write!(f, "undecodable reply: {why}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<nfs_xdr::Error> for Error {
    fn from(error: nfs_xdr::Error) -> Self {
        Self::Garbage(error.to_string())
    }
}
