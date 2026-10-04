use crate::error::ParseError;
use crate::fixed_str::FixedStr;

/// A type that decodes itself from a little-endian byte cursor.
pub trait Decode: Sized {
    fn decode(reader: &mut Reader<'_>) -> Result<Self, ParseError>;
}

macro_rules! impl_decode_primitive {
    ($($ty:ty => $method:ident),* $(,)?) => {
        $(impl Decode for $ty {
            #[inline]
            fn decode(reader: &mut Reader<'_>) -> Result<Self, ParseError> {
                reader.$method()
            }
        })*
    };
}

impl_decode_primitive! {
    u8 => read_u8,
    i8 => read_i8,
    u16 => read_u16_le,
    i16 => read_i16_le,
    u32 => read_u32_le,
    i32 => read_i32_le,
    u64 => read_u64_le,
    f32 => read_f32_le,
    f64 => read_f64_le,
}

impl<const N: usize> Decode for FixedStr<N> {
    #[inline]
    fn decode(reader: &mut Reader<'_>) -> Result<Self, ParseError> {
        reader.read_fixed_str()
    }
}

/// A cursor over a byte slice that reads little-endian values sequentially.
///
/// All reads advance the internal position. If a scalar or byte-array read
/// would exceed the remaining bytes, `ParseError::BufferTooShort` is returned
/// and the position is **not** advanced; see [`Reader::read_array`] for the
/// element-wise exception.
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

    /// Read a fixed-size, NUL-padded string buffer of `N` bytes without allocating.
    #[inline]
    pub fn read_fixed_str<const N: usize>(&mut self) -> Result<FixedStr<N>, ParseError> {
        Ok(FixedStr::from_bytes(self.read_u8_array::<N>()?))
    }

    /// Read a fixed-size array of `u8`.
    #[inline]
    pub fn read_u8_array<const N: usize>(&mut self) -> Result<[u8; N], ParseError> {
        let b = self.take(N)?;
        let mut arr = [0u8; N];
        arr.copy_from_slice(b);
        Ok(arr)
    }

    /// Read a fixed-size array of any decodable element type.
    ///
    /// Elements are decoded one by one: on error the position is left after
    /// the elements that were read successfully.
    #[inline]
    pub fn read_array<T: Decode + Default + Copy, const N: usize>(
        &mut self,
    ) -> Result<[T; N], ParseError> {
        let mut arr = [T::default(); N];
        for v in &mut arr {
            *v = T::decode(self)?;
        }
        Ok(arr)
    }

    /// Read a little-endian `u16` at the current position without advancing.
    #[inline]
    pub fn peek_u16_le(&self) -> Result<u16, ParseError> {
        if self.remaining() < 2 {
            return Err(ParseError::BufferTooShort {
                needed: 2,
                available: self.remaining(),
            });
        }
        Ok(u16::from_le_bytes([
            self.buf[self.pos],
            self.buf[self.pos + 1],
        ]))
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
    fn read_fixed_str_nul_terminated() {
        let mut data = [0u8; 48];
        data[0] = b'M';
        data[1] = b'a';
        data[2] = b'x';
        // rest is NUL
        let mut r = Reader::new(&data);
        assert_eq!(r.read_fixed_str::<48>().unwrap().as_str().unwrap(), "Max");
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

    #[test]
    fn peek_u16_le_does_not_advance() {
        let data = [0xE9, 0x07, 0x01];
        let mut r = Reader::new(&data);
        assert_eq!(r.peek_u16_le().unwrap(), 2025);
        assert_eq!(r.position(), 0);
        r.skip(2).unwrap();
        assert_eq!(
            r.peek_u16_le(),
            Err(ParseError::BufferTooShort {
                needed: 2,
                available: 1
            })
        );
        assert_eq!(r.position(), 2);
    }

    #[test]
    fn read_array_decodes_in_order() {
        let data = [0x01, 0x00, 0x02, 0x00, 0xFF, 0xFF];
        let mut r = Reader::new(&data);
        assert_eq!(r.read_array::<u16, 2>().unwrap(), [1, 2]);
        assert_eq!(r.read_array::<i16, 1>().unwrap(), [-1]);
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn read_array_truncated() {
        let data = [0x01, 0x00, 0x02];
        let mut r = Reader::new(&data);
        assert_eq!(
            r.read_array::<u16, 2>(),
            Err(ParseError::BufferTooShort {
                needed: 2,
                available: 1
            })
        );
        assert_eq!(r.position(), 2);
    }
}
