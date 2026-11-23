use crate::error::AsteropeError;

/// Little-endian cursor over a byte slice; running out of bytes yields `error`.
pub(crate) struct Reader<'a> {
    pub bytes: &'a [u8],
    pub error: AsteropeError,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8], error: AsteropeError) -> Reader<'a> {
        Reader { bytes, error }
    }

    pub fn take<const N: usize>(&mut self) -> Result<[u8; N], AsteropeError> {
        let (head, rest) = self.bytes.split_first_chunk::<N>().ok_or(self.error)?;
        self.bytes = rest;
        Ok(*head)
    }

    pub fn u16(&mut self) -> Result<u16, AsteropeError> {
        self.take().map(u16::from_le_bytes)
    }

    pub fn i16(&mut self) -> Result<i16, AsteropeError> {
        self.take().map(i16::from_le_bytes)
    }

    pub fn u32(&mut self) -> Result<u32, AsteropeError> {
        self.take().map(u32::from_le_bytes)
    }

    pub fn f32(&mut self) -> Result<f32, AsteropeError> {
        self.take().map(f32::from_le_bytes)
    }

    pub fn f64(&mut self) -> Result<f64, AsteropeError> {
        self.take().map(f64::from_le_bytes)
    }
}

/// CRC-32 of a header whose own check field, the four bytes at `field`, is
/// taken as zero. `None` when the header is shorter than `len`.
pub(crate) fn header_crc(bytes: &[u8], len: usize, field: usize) -> Option<u32> {
    let before = bytes.get(..field)?;
    let after = bytes.get(field + 4..len)?;
    let mut crc = 0xFFFF_FFFFu32;
    for byte in before.iter().chain([0u8; 4].iter()).chain(after.iter()) {
        crc = step(crc, *byte);
    }
    Some(!crc)
}

/// The stored header check: absent (zero, as files written before it
/// existed have it) or equal to the computed one.
pub(crate) fn header_intact(bytes: &[u8], len: usize, field: usize) -> bool {
    let stored = bytes
        .get(field..field + 4)
        .and_then(|b| <[u8; 4]>::try_from(b).ok())
        .map(u32::from_le_bytes);
    match stored {
        Some(0) => true,
        Some(value) => header_crc(bytes, len, field) == Some(value),
        None => false,
    }
}

fn step(mut crc: u32, byte: u8) -> u32 {
    crc ^= u32::from(byte);
    for _ in 0..8 {
        let mask = (crc & 1).wrapping_neg();
        crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
    }
    crc
}

pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for byte in bytes {
        crc = step(crc, *byte);
    }
    !crc
}
