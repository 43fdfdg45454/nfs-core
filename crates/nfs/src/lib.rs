//! NFSv4.2 client: one session over TCP or HTTP/3 CONNECT tunnels, with RPC-with-TLS.

pub mod attr;
mod callback;
mod client;
mod compound;
mod config;
mod error;
#[cfg(feature = "fuzzing")]
#[doc(hidden)]
pub mod fuzzing;
mod ops;
mod rate;
mod session;
mod stats;
mod status;
mod transport;
mod types;

pub use client::{Client, File, OpenFile};
pub use config::Config;
pub use error::{Error, Result};
pub use ops::dir::Entry;
pub use ops::lock::LockKind;
pub use ops::open::Create;
pub use ops::open::{READ_ACCESS, WRITE_ACCESS};
pub use rate::Rate;
pub use stats::Stats;
pub use status::Status;
pub use transport::{Security, Transport};
pub use types::{Fh, Stateid, Time};
