use crate::error::ParseError;
use crate::reader::{Decode, Reader};

/// The 29-byte packet header shared by all F1 telemetry packets across all supported years.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PacketHeader {
    /// The packet format identifier (e.g. 2023, 2024, 2025).
    pub packet_format: u16,
    /// Last two digits of the game year (e.g. 23, 24, 25).
    pub game_year: u8,
    /// Game major version ("X.00").
    pub game_major_version: u8,
    /// Game minor version ("1.XX").
    pub game_minor_version: u8,
    /// Version of this packet type (starts from 1).
    pub packet_version: u8,
    /// Identifier for the packet type (0-15).
    pub packet_id: u8,
    /// Unique session identifier.
    pub session_uid: u64,
    /// Session timestamp in seconds.
    pub session_time: f32,
    /// Frame identifier (resets on flashback).
    pub frame_identifier: u32,
    /// Overall frame identifier (does not reset on flashback).
    pub overall_frame_identifier: u32,
    /// Index of the player's car in car arrays.
    pub player_car_index: u8,
    /// Index of secondary player's car (splitscreen), 255 if none.
    pub secondary_player_car_index: u8,
}

/// Header is always 29 bytes.
pub const HEADER_SIZE: usize = 29;

impl Decode for PacketHeader {
    /// Parse a packet header from the front of a byte slice.
    fn decode(reader: &mut Reader<'_>) -> Result<Self, ParseError> {
        Ok(Self {
            packet_format: reader.read_u16_le()?,
            game_year: reader.read_u8()?,
            game_major_version: reader.read_u8()?,
            game_minor_version: reader.read_u8()?,
            packet_version: reader.read_u8()?,
            packet_id: reader.read_u8()?,
            session_uid: reader.read_u64_le()?,
            session_time: reader.read_f32_le()?,
            frame_identifier: reader.read_u32_le()?,
            overall_frame_identifier: reader.read_u32_le()?,
            player_car_index: reader.read_u8()?,
            secondary_player_car_index: reader.read_u8()?,
        })
    }
}

/// Parse only the header from a raw UDP datagram.
pub fn parse_header(data: &[u8]) -> Result<PacketHeader, ParseError> {
    if data.len() < HEADER_SIZE {
        return Err(ParseError::BufferTooShort {
            needed: HEADER_SIZE,
            available: data.len(),
        });
    }
    let mut reader = Reader::new(data);
    PacketHeader::decode(&mut reader)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_header_bytes(format: u16, year: u8, packet_id: u8) -> [u8; HEADER_SIZE] {
        let mut buf = [0u8; HEADER_SIZE];
        buf[0..2].copy_from_slice(&format.to_le_bytes());
        buf[2] = year;
        buf[3] = 1; // major
        buf[4] = 0; // minor
        buf[5] = 1; // packet version
        buf[6] = packet_id;
        // session_uid = 0 (bytes 7..15)
        // session_time = 0.0 (bytes 15..19)
        // frame_identifier = 0 (bytes 19..23)
        // overall_frame_identifier = 0 (bytes 23..27)
        buf[27] = 0; // player_car_index
        buf[28] = 255; // secondary_player_car_index
        buf
    }

    #[test]
    fn parse_f1_23_header() {
        let buf = make_header_bytes(2023, 23, 0);
        let h = parse_header(&buf).unwrap();
        assert_eq!(h.packet_format, 2023);
        assert_eq!(h.game_year, 23);
        assert_eq!(h.packet_id, 0);
        assert_eq!(h.secondary_player_car_index, 255);
    }

    #[test]
    fn parse_f1_24_header() {
        let buf = make_header_bytes(2024, 24, 14);
        let h = parse_header(&buf).unwrap();
        assert_eq!(h.packet_format, 2024);
        assert_eq!(h.packet_id, 14);
    }

    #[test]
    fn parse_f1_25_header() {
        let buf = make_header_bytes(2025, 25, 15);
        let h = parse_header(&buf).unwrap();
        assert_eq!(h.packet_format, 2025);
        assert_eq!(h.packet_id, 15);
    }

    #[test]
    fn header_too_short() {
        let buf = [0u8; 10];
        assert!(matches!(
            parse_header(&buf),
            Err(ParseError::BufferTooShort { .. })
        ));
    }
}
