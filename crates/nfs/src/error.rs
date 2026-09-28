use crate::status::Status;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The server answered with an error status for this operation.
    Nfs(Status),
    /// No answer, or a refusal below NFS; see nfs_rpc::Error.
    Rpc(nfs_rpc::Error),
    /// A modifying operation was sent, the connection dropped, and the server no longer knows
    /// whether it ran it (its reply was not cached). Its effect is unknown.
    Uncertain,
    /// Anything else, with a reason.
    Other(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Nfs(status) => write!(f, "{status}"),
            Self::Rpc(error) => write!(f, "{error}"),
            Self::Uncertain => f.write_str("the connection dropped and the outcome is unknown"),
            Self::Other(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for Error {}

impl From<nfs_rpc::Error> for Error {
    fn from(error: nfs_rpc::Error) -> Self {
        Self::Rpc(error)
    }
}

impl From<nfs_xdr::Error> for Error {
    fn from(error: nfs_xdr::Error) -> Self {
        Self::Rpc(error.into())
    }
}

impl From<Error> for std::io::Error {
    fn from(error: Error) -> Self {
        let kind = match &error {
            Error::Nfs(status) => status.kind(),
            Error::Rpc(nfs_rpc::Error::Stalled) => std::io::ErrorKind::TimedOut,
            Error::Rpc(_) => std::io::ErrorKind::ConnectionAborted,
            _ => std::io::ErrorKind::Other,
        };
        Self::new(kind, error)
    }
}

pub type Result<T> = std::result::Result<T, Error>;
