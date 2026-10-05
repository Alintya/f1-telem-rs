//! Capture files: raw UDP datagrams recorded from the game, for offline replay and
//! regression tests. Private to the tooling: `f1-listen` (`--record`/`--replay`) and
//! `tests/captures.rs` both include it with `#[path]`; it is not part of the `f1_telem`
//! library API.
//!
//! Format: the 8-byte [`MAGIC`], then for every datagram its length as a little-endian
//! `u16` followed by its bytes.

use std::io::{self, Write};

/// File signature and format version.
pub const MAGIC: [u8; 8] = *b"F1TCAP01";

/// Appends datagrams to a capture.
///
/// Wrap files in a `BufWriter` and call [`flush`](Self::flush) after each datagram if the
/// recording may be interrupted (e.g. by Ctrl+C); a record cut short is reported by
/// [`datagrams`] as an error after all complete records.
pub struct CaptureWriter<W: Write> {
    inner: W,
}

impl<W: Write> CaptureWriter<W> {
    /// Start a capture by writing the file signature.
    pub fn new(mut inner: W) -> io::Result<Self> {
        inner.write_all(&MAGIC)?;
        Ok(Self { inner })
    }

    /// Append one datagram. Fails with `InvalidInput` if it is longer than `u16::MAX` bytes.
    pub fn write(&mut self, datagram: &[u8]) -> io::Result<()> {
        let len = u16::try_from(datagram.len()).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("datagram of {} bytes exceeds {}", datagram.len(), u16::MAX),
            )
        })?;
        self.inner.write_all(&len.to_le_bytes())?;
        self.inner.write_all(datagram)
    }

    /// Flush the underlying writer.
    pub fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Iterate over the datagrams of a capture held in memory, without copying.
///
/// Fails with `InvalidData` if `capture` does not start with [`MAGIC`].
pub fn datagrams(capture: &[u8]) -> io::Result<Datagrams<'_>> {
    match capture.strip_prefix(&MAGIC) {
        Some(rest) => Ok(Datagrams { rest }),
        None => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "not an f1-telem capture (bad signature)",
        )),
    }
}

/// Iterator returned by [`datagrams`]. Yields one `InvalidData` error and then stops if
/// the last record is truncated.
pub struct Datagrams<'a> {
    rest: &'a [u8],
}

impl<'a> Iterator for Datagrams<'a> {
    type Item = io::Result<&'a [u8]>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.rest.is_empty() {
            return None;
        }
        let record = match *self.rest {
            [lo, hi, ref body @ ..] => {
                let len = usize::from(u16::from_le_bytes([lo, hi]));
                body.get(..len).map(|d| (d, &body[len..]))
            }
            _ => None,
        };
        match record {
            Some((datagram, rest)) => {
                self.rest = rest;
                Some(Ok(datagram))
            }
            None => {
                let left = self.rest.len();
                self.rest = &[];
                Some(Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("truncated record at end of capture ({left} bytes left)"),
                )))
            }
        }
    }
}
