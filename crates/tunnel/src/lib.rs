//! What the gateway and the client share: TLS and QUIC settings, and the copy between an HTTP/3
//! `CONNECT` stream (RFC 9114 section 4.4) and the TCP connection it carries.

pub mod args;
pub mod client;
pub mod congestion;
pub mod pump;
pub mod quic;
pub mod tls;

/// Boxed error for the binaries and the setup code; the data path reports plain I/O errors.
pub type Error = Box<dyn std::error::Error + Send + Sync>;
