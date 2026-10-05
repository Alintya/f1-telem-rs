use std::fmt;

/// All errors that can occur when parsing F1 telemetry packets.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ParseError {
    /// Not enough bytes to complete the read.
    /// Contains the number of bytes needed and the number available.
    BufferTooShort { needed: usize, available: usize },

    /// The packet format (year) in the header is not supported.
    UnsupportedFormat(u16),

    /// The packet id is not recognized for the given game version.
    UnknownPacketId { packet_id: u8, format: u16 },

    /// The actual packet size did not match the expected size for this packet type.
    SizeMismatch { expected: usize, actual: usize },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BufferTooShort { needed, available } => {
                write!(f, "buffer too short: need {needed} bytes, have {available}")
            }
            Self::UnsupportedFormat(fmt_val) => {
                write!(f, "unsupported packet format: {fmt_val}")
            }
            Self::UnknownPacketId { packet_id, format } => {
                write!(f, "unknown packet id {packet_id} for format {format}")
            }
            Self::SizeMismatch { expected, actual } => {
                write!(f, "packet size mismatch: expected {expected}, got {actual}")
            }
        }
    }
}

impl std::error::Error for ParseError {}
