//! Call and reply messages (RFC 5531 section 9) and the credentials NFS uses.

use crate::{NFS_PROGRAM, NFS_V4};
use bytes::Bytes;
use nfs_xdr::Encoder;

/// AUTH_SYS (RFC 5531 appendix A): the identity the server applies its export rules to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SysCred {
    pub machine: String,
    pub uid: u32,
    pub gid: u32,
    /// At most 16 are sent.
    pub gids: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Auth {
    None,
    Sys(SysCred),
    /// AUTH_TLS (RFC 9289): only on the NULL call that asks for TLS.
    Tls,
}

impl Auth {
    fn encode(&self, e: &mut Encoder) {
        match self {
            Self::None => e.u32(0).u32(0),
            Self::Tls => e.u32(7).u32(0),
            Self::Sys(cred) => {
                let mut body = Encoder::new();
                body.u32(0).string(&cred.machine).u32(cred.uid).u32(cred.gid);
                body.bitmap(&cred.gids[..cred.gids.len().min(16)]);
                e.u32(1).opaque(&body.finish())
            }
        };
    }
}

/// A record-marked call to NFSv4 procedure `procedure`, its arguments written by `args`.
pub fn call(xid: u32, procedure: u32, auth: &Auth, args: impl FnOnce(&mut Encoder)) -> Bytes {
    let mut e = Encoder::new();
    e.u32(0).u32(xid).u32(0).u32(2).u32(NFS_PROGRAM).u32(NFS_V4).u32(procedure);
    auth.encode(&mut e);
    e.u32(0).u32(0);
    args(&mut e);
    let len = e.len() as u32 - 4;
    e.patch_u32(0, 0x8000_0000 | len);
    e.finish()
}
