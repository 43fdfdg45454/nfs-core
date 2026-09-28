//! XDR (RFC 4506), as much as NFSv4 needs. Decoding is zero-copy: opaque data (a READ's payload)
//! is a slice of the received buffer.

mod decode;
mod encode;

pub use decode::Decoder;
pub use encode::Encoder;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The data ends before the value does.
    Truncated,
    /// A length or a string that cannot be what it claims to be.
    Invalid,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Truncated => "XDR data truncated",
            Self::Invalid => "invalid XDR data",
        })
    }
}

impl std::error::Error for Error {}

/// Bytes of padding after `len` bytes of opaque data.
fn padding(len: usize) -> usize {
    (4 - len % 4) % 4
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut e = Encoder::new();
        e.u32(7).u64(1 << 40).bool(true).opaque(b"abcde").string("nfs").opaque_fixed(&[1; 8]);
        e.bitmap(&[0x1a, 0, 3]);
        let bytes = e.finish();
        assert_eq!(bytes.len(), 4 + 8 + 4 + 12 + 8 + 8 + 16);
        let mut d = Decoder::new(bytes);
        assert_eq!(d.u32(), Ok(7));
        assert_eq!(d.u64(), Ok(1 << 40));
        assert_eq!(d.bool(), Ok(true));
        assert_eq!(&d.opaque().unwrap()[..], b"abcde");
        assert_eq!(d.string().as_deref(), Ok("nfs"));
        assert_eq!(&d.opaque_fixed(8).unwrap()[..], &[1; 8]);
        assert_eq!(d.bitmap(), Ok(vec![0x1a, 0, 3]));
        assert_eq!(d.u32(), Err(Error::Truncated));
    }
}
