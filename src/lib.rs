pub mod error;
pub mod fixed_str;
pub mod header;
pub mod reader;

mod generated;

// Generated from spec/*.yaml: the `v20XX` modules plus `GameVersion`,
// `PacketKind` and the version-tagged `Packet` enum.
pub use generated::*;

pub use error::ParseError;
pub use fixed_str::FixedStr;
pub use header::{parse_header, PacketHeader, HEADER_SIZE};

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
    Packet::decode(&mut reader::Reader::new(data))
}
