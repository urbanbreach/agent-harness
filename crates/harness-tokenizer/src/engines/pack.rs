//! Readers for the bundled zstd vocabulary payloads.

use std::io::Read;

/// Decompress one bundled asset.
pub(super) fn decompress(packed: &[u8]) -> Option<Vec<u8>> {
    let mut decoder = ruzstd::decoding::StreamingDecoder::new(packed).ok()?;
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).ok()?;
    Some(out)
}

/// Forward-only cursor over a decompressed payload.
pub(super) struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub(super) const fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub(super) fn bytes(&mut self, len: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(len)?;
        let slice = self.data.get(self.pos..end)?;
        self.pos = end;
        Some(slice)
    }

    pub(super) fn u32_le(&mut self) -> Option<u32> {
        let raw: [u8; 4] = self.bytes(4)?.try_into().ok()?;
        Some(u32::from_le_bytes(raw))
    }

    /// LEB128 unsigned integer.
    pub(super) fn varint(&mut self) -> Option<u64> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = *self.data.get(self.pos)?;
            self.pos += 1;
            value |= u64::from(byte & 0x7F) << shift;
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }

    pub(super) fn varint_len(&mut self) -> Option<usize> {
        usize::try_from(self.varint()?).ok()
    }

    pub(super) const fn is_empty(&self) -> bool {
        self.pos == self.data.len()
    }
}
