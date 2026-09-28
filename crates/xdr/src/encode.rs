use bytes::{BufMut, Bytes, BytesMut};

/// Appends XDR values to a buffer; methods chain.
#[derive(Default)]
pub struct Encoder(BytesMut);

impl Encoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn u32(&mut self, value: u32) -> &mut Self {
        self.0.put_u32(value);
        self
    }

    pub fn u64(&mut self, value: u64) -> &mut Self {
        self.0.put_u64(value);
        self
    }

    pub fn i64(&mut self, value: i64) -> &mut Self {
        self.0.put_i64(value);
        self
    }

    pub fn bool(&mut self, value: bool) -> &mut Self {
        self.u32(value.into())
    }

    /// Fixed-length opaque data: the bytes and their padding.
    pub fn opaque_fixed(&mut self, data: &[u8]) -> &mut Self {
        self.0.put_slice(data);
        self.0.put_bytes(0, super::padding(data.len()));
        self
    }

    /// Variable-length opaque data: its length, the bytes and their padding.
    pub fn opaque(&mut self, data: &[u8]) -> &mut Self {
        self.u32(data.len() as u32).opaque_fixed(data)
    }

    pub fn string(&mut self, value: &str) -> &mut Self {
        self.opaque(value.as_bytes())
    }

    /// An array of 32-bit words, such as NFSv4's bitmap4.
    pub fn bitmap(&mut self, words: &[u32]) -> &mut Self {
        self.u32(words.len() as u32);
        words.iter().for_each(|&w| self.0.put_u32(w));
        self
    }

    /// Overwrites a 32-bit word written earlier at `at` (a count known only later).
    pub fn patch_u32(&mut self, at: usize, value: u32) {
        self.0[at..at + 4].copy_from_slice(&value.to_be_bytes());
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    pub fn finish(self) -> Bytes {
        self.0.freeze()
    }
}
