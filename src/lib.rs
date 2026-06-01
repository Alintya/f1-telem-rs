pub mod error;
pub mod header;
pub mod reader;
pub mod version;

mod generated;

// Re-export generated version modules at crate root.
pub use generated::v2023;
pub use generated::v2024;
pub use generated::v2025;

pub use error::ParseError;
pub use header::{parse_header, PacketHeader, HEADER_SIZE};
pub use version::{GameVersion, PacketKind};

/// A parsed F1 telemetry packet, tagged by game version.
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum Packet {
    F1_23(v2023::Packet),
    F1_24(v2024::Packet),
    F1_25(v2025::Packet),
}

impl Packet {
    /// The packet header.
    pub fn header(&self) -> &PacketHeader {
        match self {
            Self::F1_23(p) => p.header(),
            Self::F1_24(p) => p.header(),
            Self::F1_25(p) => p.header(),
        }
    }

    /// The game version this packet came from.
    pub fn version(&self) -> GameVersion {
        match self {
            Self::F1_23(_) => GameVersion::F1_23,
            Self::F1_24(_) => GameVersion::F1_24,
            Self::F1_25(_) => GameVersion::F1_25,
        }
    }

    /// The packet kind.
    pub fn kind(&self) -> PacketKind {
        match self {
            Self::F1_23(p) => p.kind(),
            Self::F1_24(p) => p.kind(),
            Self::F1_25(p) => p.kind(),
        }
    }
}

/// Parse a complete F1 telemetry UDP datagram into a typed packet.
///
/// Reads the format field to determine game version, then dispatches
/// to the version-specific decoder which parses header + body in one pass.
pub fn parse_packet(data: &[u8]) -> Result<Packet, ParseError> {
    if data.len() < HEADER_SIZE {
        return Err(ParseError::BufferTooShort {
            needed: HEADER_SIZE,
            available: data.len(),
        });
    }
    let format = u16::from_le_bytes([data[0], data[1]]);
    let mut r = reader::Reader::new(data);

    match format {
        2023 => Ok(Packet::F1_23(v2023::Packet::decode(&mut r)?)),
        2024 => Ok(Packet::F1_24(v2024::Packet::decode(&mut r)?)),
        2025 => Ok(Packet::F1_25(v2025::Packet::decode(&mut r)?)),
        other => Err(ParseError::UnsupportedFormat(other)),
    }
}
