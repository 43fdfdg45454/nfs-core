use super::Error;
use bytes::Bytes;

/// Reads XDR values in order from a received buffer.
pub struct Decoder {
    data: Bytes,
    at: usize,
}

type Result<T> = std::result::Result<T, Error>;

/// Longest variable-length item accepted: more than any NFS reply carries.
const MAX_OPAQUE: usize = 64 << 20;

impl Decoder {
    pub fn new(data: Bytes) -> Self {
        Self { data, at: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.data.len() - self.at
    }

    fn take(&mut self, len: usize) -> Result<Bytes> {
        if self.remaining() < len {
            return Err(Error::Truncated);
        }
        self.at += len;
        Ok(self.data.slice(self.at - len..self.at))
    }

    pub fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?[..].try_into().unwrap_or_default()))
    }

    pub fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.take(8)?[..].try_into().unwrap_or_default()))
    }

    pub fn i64(&mut self) -> Result<i64> {
        self.u64().map(|v| v as i64)
    }

    pub fn bool(&mut self) -> Result<bool> {
        self.u32().map(|v| v != 0)
    }

    pub fn opaque_fixed(&mut self, len: usize) -> Result<Bytes> {
        let data = self.take(len)?;
        self.take(super::padding(len))?;
        Ok(data)
    }

    pub fn opaque(&mut self) -> Result<Bytes> {
        let len = self.u32()? as usize;
        if len > MAX_OPAQUE {
            return Err(Error::Invalid);
        }
        self.opaque_fixed(len)
    }

    pub fn string(&mut self) -> Result<String> {
        String::from_utf8(self.opaque()?.to_vec()).map_err(|_| Error::Invalid)
    }

    pub fn bitmap(&mut self) -> Result<Vec<u32>> {
        let len = self.u32()? as usize;
        if len > self.remaining() / 4 {
            return Err(Error::Truncated);
        }
        (0..len).map(|_| self.u32()).collect()
    }

    /// The rest of the buffer, undecoded.
    pub fn rest(&mut self) -> Bytes {
        let len = self.remaining();
        self.take(len).unwrap_or_default()
    }
}
