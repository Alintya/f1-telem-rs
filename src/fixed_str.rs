use std::borrow::Cow;
use std::fmt;
use std::hash::{Hash, Hasher};

/// Fixed-capacity, NUL-terminated UTF-8 string stored inline (no allocation).
///
/// The game sends names as fixed-size byte buffers padded with NUL. The raw
/// bytes are kept as-is; UTF-8 validation is deferred to [`FixedStr::as_str`]
/// so one malformed name never fails a whole packet.
///
/// Equality, hashing and display consider only the bytes before the first NUL;
/// anything after the terminator is visible only through [`FixedStr::raw`].
#[derive(Clone, Copy)]
pub struct FixedStr<const N: usize>([u8; N]);

impl<const N: usize> FixedStr<N> {
    /// Wrap a raw NUL-padded buffer as received on the wire.
    pub const fn from_bytes(bytes: [u8; N]) -> Self {
        Self(bytes)
    }

    /// Bytes before the first NUL (or all `N` bytes if there is no NUL).
    pub fn as_bytes(&self) -> &[u8] {
        let end = self.0.iter().position(|&b| b == 0).unwrap_or(N);
        &self.0[..end]
    }

    /// Full backing buffer, including the NUL terminator and any bytes after it.
    pub const fn raw(&self) -> &[u8; N] {
        &self.0
    }

    /// The string contents, if they are valid UTF-8.
    pub fn as_str(&self) -> Result<&str, std::str::Utf8Error> {
        std::str::from_utf8(self.as_bytes())
    }

    /// The string contents, replacing invalid UTF-8 sequences with `U+FFFD`.
    /// Borrows (no allocation) when the contents are valid UTF-8.
    pub fn to_string_lossy(&self) -> Cow<'_, str> {
        String::from_utf8_lossy(self.as_bytes())
    }

    /// Length in bytes of the contents before the first NUL.
    pub fn len(&self) -> usize {
        self.as_bytes().len()
    }

    /// Whether the contents before the first NUL are empty.
    pub const fn is_empty(&self) -> bool {
        N == 0 || self.0[0] == 0
    }
}

impl<const N: usize> Default for FixedStr<N> {
    fn default() -> Self {
        Self([0; N])
    }
}

impl<const N: usize> PartialEq for FixedStr<N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl<const N: usize> Eq for FixedStr<N> {}

impl<const N: usize> Hash for FixedStr<N> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_bytes().hash(state);
    }
}

impl<const N: usize> AsRef<[u8]> for FixedStr<N> {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl<const N: usize> From<[u8; N]> for FixedStr<N> {
    fn from(bytes: [u8; N]) -> Self {
        Self(bytes)
    }
}

impl<const N: usize> fmt::Debug for FixedStr<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.to_string_lossy(), f)
    }
}

impl<const N: usize> fmt::Display for FixedStr<N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(&self.to_string_lossy())
    }
}

impl<const N: usize> PartialEq<str> for FixedStr<N> {
    fn eq(&self, other: &str) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl<const N: usize> PartialEq<&str> for FixedStr<N> {
    fn eq(&self, other: &&str) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl<const N: usize> PartialEq<FixedStr<N>> for str {
    fn eq(&self, other: &FixedStr<N>) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl<const N: usize> PartialEq<FixedStr<N>> for &str {
    fn eq(&self, other: &FixedStr<N>) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

/// Serializes as a string: the contents before the first NUL, invalid UTF-8 replaced by `U+FFFD`.
#[cfg(feature = "serde")]
impl<const N: usize> serde::Serialize for FixedStr<N> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string_lossy())
    }
}

/// Deserializes from a string without allocating: the bytes before the first NUL are
/// copied into the buffer and the rest is NUL-padded. Strings longer than `N` bytes are
/// truncated at the last character boundary that fits, as the game does with long names.
#[cfg(feature = "serde")]
impl<'de, const N: usize> serde::Deserialize<'de> for FixedStr<N> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct FixedStrVisitor<const N: usize>;

        impl<const N: usize> serde::de::Visitor<'_> for FixedStrVisitor<N> {
            type Value = FixedStr<N>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "a string of at most {N} bytes")
            }

            fn visit_str<E: serde::de::Error>(self, s: &str) -> Result<FixedStr<N>, E> {
                let content = s.split('\0').next().unwrap_or_default();
                let mut end = content.len().min(N);
                while !content.is_char_boundary(end) {
                    end -= 1;
                }
                let mut buf = [0u8; N];
                buf[..end].copy_from_slice(&content.as_bytes()[..end]);
                Ok(FixedStr(buf))
            }
        }

        deserializer.deserialize_str(FixedStrVisitor::<N>)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::hash_map::DefaultHasher;

    fn padded<const N: usize>(s: &[u8]) -> FixedStr<N> {
        let mut buf = [0u8; N];
        buf[..s.len()].copy_from_slice(s);
        FixedStr::from_bytes(buf)
    }

    fn hash_of<T: Hash>(v: &T) -> u64 {
        let mut h = DefaultHasher::new();
        v.hash(&mut h);
        h.finish()
    }

    #[test]
    fn nul_terminated() {
        let s: FixedStr<48> = padded(b"Max");
        assert_eq!(s.as_str(), Ok("Max"));
        assert_eq!(s.len(), 3);
        assert_eq!(s, "Max");
        assert_eq!("Max", s);
        assert!(matches!(s.to_string_lossy(), Cow::Borrowed("Max")));
    }

    #[test]
    fn no_nul_uses_whole_buffer() {
        let s = FixedStr::from_bytes(*b"abcd");
        assert_eq!(s.as_str(), Ok("abcd"));
        assert_eq!(s.len(), 4);
    }

    #[test]
    fn bytes_after_nul_are_ignored_except_raw() {
        let clean: FixedStr<8> = padded(b"Max");
        let dirty = FixedStr::from_bytes(*b"Max\0junk");
        assert_eq!(dirty.as_str(), Ok("Max"));
        assert_eq!(dirty.len(), 3);
        assert_eq!(dirty, clean);
        assert_eq!(hash_of(&dirty), hash_of(&clean));
        assert_eq!(dirty.raw(), b"Max\0junk");
    }

    #[test]
    fn invalid_utf8_is_deferred() {
        let s: FixedStr<8> = padded(&[0xFF, b'a']);
        assert!(s.as_str().is_err());
        assert_eq!(s.to_string_lossy(), "\u{FFFD}a");
        assert!(!s.is_empty());
    }

    #[test]
    fn default_is_empty() {
        let s = FixedStr::<4>::default();
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
        assert_eq!(s, "");
        assert!(FixedStr::<0>::default().is_empty());
    }

    #[test]
    fn formatting() {
        let s: FixedStr<8> = padded(b"Max");
        assert_eq!(format!("{s:?}"), "\"Max\"");
        assert_eq!(format!("[{s:>5}]"), "[  Max]");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_serializes_as_lossy_string() {
        assert_eq!(
            serde_json::to_string(&padded::<8>(b"Max")).unwrap(),
            "\"Max\""
        );
        let invalid: FixedStr<8> = padded(&[0xFF, b'a']);
        assert_eq!(serde_json::to_string(&invalid).unwrap(), "\"\u{FFFD}a\"");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_deserializes_nul_padded() {
        let s: FixedStr<8> = serde_json::from_str("\"Max\"").unwrap();
        assert_eq!(s.raw(), b"Max\0\0\0\0\0");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_truncates_at_char_boundary() {
        // '€' is 3 bytes and would straddle the 4-byte limit.
        let s: FixedStr<4> = serde_json::from_str("\"ab€cd\"").unwrap();
        assert_eq!(s, "ab");
        let exact: FixedStr<4> = serde_json::from_str("\"abcd\"").unwrap();
        assert_eq!(exact, "abcd");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_stops_at_interior_nul() {
        let s: FixedStr<8> = serde_json::from_str("\"Ma\\u0000x\"").unwrap();
        assert_eq!(s.raw(), b"Ma\0\0\0\0\0\0");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_rejects_non_strings() {
        let err = serde_json::from_str::<FixedStr<8>>("42").unwrap_err();
        assert!(
            err.to_string().contains("a string of at most 8 bytes"),
            "{err}"
        );
    }
}
