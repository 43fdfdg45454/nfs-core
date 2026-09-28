//! NFSv4 status codes (RFC 7530, 8881, 7862): the ones the client acts on, and names for all
//! the others it may show.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status(pub u32);

macro_rules! statuses {
    ($($name:ident = $value:literal,)*) => {
        impl Status { $(pub const $name: Status = Status($value);)* }
        impl std::fmt::Display for Status {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self.0 { $($value => f.write_str(concat!("NFS4ERR_", stringify!($name))),)* n => write!(f, "NFS4ERR {n}") }
            }
        }
    };
}

statuses! {
    OK = 0, PERM = 1, NOENT = 2, IO = 5, NXIO = 6, ACCESS = 13, EXIST = 17, XDEV = 18,
    NOTDIR = 20, ISDIR = 21, INVAL = 22, FBIG = 27, NOSPC = 28, ROFS = 30, MLINK = 31,
    NAMETOOLONG = 63, NOTEMPTY = 66, DQUOT = 69, STALE = 70, BADHANDLE = 10001,
    BAD_COOKIE = 10003, NOTSUPP = 10004, TOOSMALL = 10005, SERVERFAULT = 10006,
    BADTYPE = 10007, DELAY = 10008, SAME = 10009, DENIED = 10010, EXPIRED = 10011,
    LOCKED = 10012, GRACE = 10013, FHEXPIRED = 10014, SHARE_DENIED = 10015, WRONGSEC = 10016,
    CLID_INUSE = 10017, RESOURCE = 10018, MOVED = 10019, NOFILEHANDLE = 10020,
    MINOR_VERS_MISMATCH = 10021, STALE_CLIENTID = 10022, STALE_STATEID = 10023,
    OLD_STATEID = 10024, BAD_STATEID = 10025, BAD_SEQID = 10026, NOT_SAME = 10027,
    LOCK_RANGE = 10028, SYMLINK = 10029, RESTOREFH = 10030, LEASE_MOVED = 10031,
    ATTRNOTSUPP = 10032, NO_GRACE = 10033, RECLAIM_BAD = 10034, RECLAIM_CONFLICT = 10035,
    BADXDR = 10036, LOCKS_HELD = 10037, OPENMODE = 10038, BADOWNER = 10039, BADCHAR = 10040,
    BADNAME = 10041, BAD_RANGE = 10042, LOCK_NOTSUPP = 10043, OP_ILLEGAL = 10044,
    DEADLOCK = 10045, FILE_OPEN = 10046, ADMIN_REVOKED = 10047, CB_PATH_DOWN = 10048,
    BADSESSION = 10052, BADSLOT = 10053, COMPLETE_ALREADY = 10054,
    CONN_NOT_BOUND_TO_SESSION = 10055, SEQ_MISORDERED = 10063, SEQUENCE_POS = 10064,
    REQ_TOO_BIG = 10065, REP_TOO_BIG = 10066, REP_TOO_BIG_TO_CACHE = 10067,
    RETRY_UNCACHED_REP = 10068, TOO_MANY_OPS = 10070, OP_NOT_IN_SESSION = 10071,
    CLIENTID_BUSY = 10074, SEQ_FALSE_RETRY = 10076, BAD_HIGH_SLOT = 10077,
    DEADSESSION = 10078, NOT_ONLY_OP = 10081, WRONG_CRED = 10082, WRONG_TYPE = 10083,
    DELEG_REVOKED = 10087, OFFLOAD_DENIED = 10091, WRONG_LFS = 10092, OFFLOAD_NO_REQS = 10094,
}

impl Status {
    /// The open state behind a stateid is gone: reopening the file fixes it.
    pub fn lost_state(self) -> bool {
        [
            Self::BAD_STATEID,
            Self::EXPIRED,
            Self::ADMIN_REVOKED,
            Self::DELEG_REVOKED,
            Self::STALE_STATEID,
            Self::OLD_STATEID,
        ]
        .contains(&self)
    }

    /// The session or the client is gone: set up anew.
    pub fn lost_session(self) -> bool {
        [Self::BADSESSION, Self::DEADSESSION, Self::STALE_CLIENTID, Self::EXPIRED].contains(&self)
    }

    /// What an application would make of it.
    pub fn kind(self) -> std::io::ErrorKind {
        use std::io::ErrorKind as K;
        match self {
            Self::NOENT | Self::STALE => K::NotFound,
            Self::PERM | Self::ACCESS | Self::WRONGSEC => K::PermissionDenied,
            Self::EXIST => K::AlreadyExists,
            Self::NOTDIR => K::NotADirectory,
            Self::ISDIR => K::IsADirectory,
            Self::NOTEMPTY => K::DirectoryNotEmpty,
            Self::NOSPC | Self::DQUOT => K::StorageFull,
            Self::ROFS => K::ReadOnlyFilesystem,
            Self::NAMETOOLONG => K::InvalidFilename,
            Self::FBIG => K::FileTooLarge,
            Self::XDEV => K::CrossesDevices,
            Self::INVAL | Self::BADNAME | Self::BADCHAR => K::InvalidInput,
            Self::NOTSUPP | Self::ATTRNOTSUPP | Self::LOCK_NOTSUPP => K::Unsupported,
            Self::DENIED | Self::LOCKED => K::WouldBlock,
            _ => K::Other,
        }
    }
}
