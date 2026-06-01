use crate::error::ParseError;

/// A cursor over a byte slice that reads little-endian values sequentially.
///
/// All reads advance the internal position. If a read would exceed the
/// remaining bytes, `ParseError::BufferTooShort` is returned and the
/// position is **not** advanced.
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// Bytes consumed so far.
    #[inline]
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Bytes remaining.
    #[inline]
    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    /// Total buffer length.
    #[inline]
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// Whether the buffer is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// Consume exactly `n` bytes and return them as a slice.
    #[inline]
    fn take(&mut self, n: usize) -> Result<&'a [u8], ParseError> {
        if self.remaining() < n {
            return Err(ParseError::BufferTooShort {
                needed: n,
                available: self.remaining(),
            });
        }
        let start = self.pos;
        self.pos += n;
        Ok(&self.buf[start..self.pos])
    }

    #[inline]
    pub fn read_u8(&mut self) -> Result<u8, ParseError> {
        let b = self.take(1)?;
        Ok(b[0])
    }

    #[inline]
    pub fn read_i8(&mut self) -> Result<i8, ParseError> {
        Ok(self.read_u8()? as i8)
    }

    #[inline]
    pub fn read_u16_le(&mut self) -> Result<u16, ParseError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    #[inline]
    pub fn read_i16_le(&mut self) -> Result<i16, ParseError> {
        let b = self.take(2)?;
        Ok(i16::from_le_bytes([b[0], b[1]]))
    }

    #[inline]
    pub fn read_u32_le(&mut self) -> Result<u32, ParseError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    #[inline]
    pub fn read_i32_le(&mut self) -> Result<i32, ParseError> {
        let b = self.take(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    #[inline]
    pub fn read_u64_le(&mut self) -> Result<u64, ParseError> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    #[inline]
    pub fn read_f32_le(&mut self) -> Result<f32, ParseError> {
        let b = self.take(4)?;
        Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    #[inline]
    pub fn read_f64_le(&mut self) -> Result<f64, ParseError> {
        let b = self.take(8)?;
        Ok(f64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    /// Read a NUL-terminated UTF-8 string from a fixed-size buffer of `len` bytes.
    /// The full `len` bytes are always consumed from the reader.
    /// Returns everything before the first NUL byte, decoded as UTF-8.
    pub fn read_string(&mut self, len: usize, field: &'static str) -> Result<String, ParseError> {
        let b = self.take(len)?;
        let end = b.iter().position(|&c| c == 0).unwrap_or(len);
        std::str::from_utf8(&b[..end])
            .map(|s| s.to_owned())
            .map_err(|_| ParseError::InvalidUtf8 { field })
    }

    /// Read a fixed-size array of `u8`.
    pub fn read_u8_array<const N: usize>(&mut self) -> Result<[u8; N], ParseError> {
        let b = self.take(N)?;
        let mut arr = [0u8; N];
        arr.copy_from_slice(b);
        Ok(arr)
    }

    /// Read a fixed-size array of `u16` (little-endian).
    pub fn read_u16_array<const N: usize>(&mut self) -> Result<[u16; N], ParseError> {
        let mut arr = [0u16; N];
        for v in &mut arr {
            *v = self.read_u16_le()?;
        }
        Ok(arr)
    }

    /// Read a fixed-size array of `f32` (little-endian).
    pub fn read_f32_array<const N: usize>(&mut self) -> Result<[f32; N], ParseError> {
        let mut arr = [0.0f32; N];
        for v in &mut arr {
            *v = self.read_f32_le()?;
        }
        Ok(arr)
    }

    /// Read a fixed-size array of `i8`.
    pub fn read_i8_array<const N: usize>(&mut self) -> Result<[i8; N], ParseError> {
        let b = self.take(N)?;
        let mut arr = [0i8; N];
        for (i, &byte) in b.iter().enumerate() {
            arr[i] = byte as i8;
        }
        Ok(arr)
    }

    /// Skip `n` bytes without reading them.
    pub fn skip(&mut self, n: usize) -> Result<(), ParseError> {
        self.take(n)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_primitives() {
        // u16=0x0102 LE = [0x02, 0x01], u32=0x04030201 LE = [0x01,0x02,0x03,0x04]
        let data = [0x42u8, 0x02, 0x01, 0x01, 0x02, 0x03, 0x04];
        let mut r = Reader::new(&data);
        assert_eq!(r.read_u8().unwrap(), 0x42);
        assert_eq!(r.read_u16_le().unwrap(), 0x0102);
        assert_eq!(r.read_u32_le().unwrap(), 0x04030201);
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn read_string_nul_terminated() {
        let mut data = [0u8; 48];
        data[0] = b'M';
        data[1] = b'a';
        data[2] = b'x';
        // rest is NUL
        let mut r = Reader::new(&data);
        assert_eq!(r.read_string(48, "name").unwrap(), "Max");
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn buffer_too_short() {
        let data = [0u8; 3];
        let mut r = Reader::new(&data);
        assert!(matches!(
            r.read_u32_le(),
            Err(ParseError::BufferTooShort {
                needed: 4,
                available: 3
            })
        ));
        // Position should not have advanced
        assert_eq!(r.position(), 0);
    }
}
