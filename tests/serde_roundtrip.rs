#![cfg(feature = "serde")]

use f1_telem::*;
use serde_json::{json, Value};

/// A 29-byte packet header for `format`/`year`/`packet_id`.
fn header(format: u16, year: u8, packet_id: u8) -> Vec<u8> {
    let mut b = Vec::with_capacity(2048);
    b.extend_from_slice(&format.to_le_bytes());
    b.extend_from_slice(&[year, 1, 0, 1, packet_id]);
    b.extend_from_slice(&0x1234u64.to_le_bytes()); // session uid
    b.extend_from_slice(&1.5f32.to_le_bytes()); // session time
    b.extend_from_slice(&42u32.to_le_bytes()); // frame
    b.extend_from_slice(&100u32.to_le_bytes()); // overall frame
    b.extend_from_slice(&[0, 255]); // player / secondary player
    assert_eq!(b.len(), HEADER_SIZE);
    b
}

/// `bytes` zero-padded to `len`.
fn padded(mut bytes: Vec<u8>, len: usize) -> Vec<u8> {
    bytes.resize(len, 0);
    bytes
}

fn event(format: u16, year: u8, code: &[u8; 4], payload: &[u8]) -> Packet {
    let mut b = header(format, year, 3);
    b.extend_from_slice(code);
    b.extend_from_slice(payload);
    parse_packet(&padded(b, 45)).unwrap()
}

fn round_trip(packet: &Packet) {
    let text = serde_json::to_string(packet).unwrap();
    let back: Packet = serde_json::from_str(&text).unwrap();
    assert_eq!(&back, packet);
}

#[test]
fn participants_names_serialize_as_strings_and_round_trip() {
    let mut b = header(2025, 25, 4);
    b.push(1); // num_active_cars
    b.extend_from_slice(&[0; 7]); // ai_controlled .. nationality
    b.extend_from_slice(b"Lewis");
    let packet = parse_packet(&padded(b, 1284)).unwrap();

    let value = serde_json::to_value(&packet).unwrap();
    let participants = &value["F1_25"]["Participants"]["participants"];
    assert_eq!(participants.as_array().unwrap().len(), 22);
    assert_eq!(participants[0]["name"], "Lewis");
    assert_eq!(participants[1]["name"], "");

    let back: Packet = serde_json::from_value(value).unwrap();
    assert_eq!(back, packet);
}

#[test]
fn invalid_utf8_names_round_trip_lossily() {
    // Invalid bytes become U+FFFD (3 bytes), which then no longer fits and is truncated
    // at a character boundary: the round trip is not the identity.
    let name = FixedStr::<4>::from_bytes(*b"\xffabc");
    let json = serde_json::to_value(name).unwrap();
    assert_eq!(json, "\u{FFFD}abc");
    let back: FixedStr<4> = serde_json::from_value(json).unwrap();
    assert_eq!(back.as_bytes(), "\u{FFFD}a".as_bytes());
    assert_ne!(back, name);

    // A name the game cut mid-character loses the partial character.
    let cut = FixedStr::<4>::from_bytes(*b"ab\xe2\x82");
    let back: FixedStr<4> = serde_json::from_value(serde_json::to_value(cut).unwrap()).unwrap();
    assert_eq!(back, "ab");
}

#[test]
fn large_arrays_round_trip() {
    // 2026 LapPositions: 50 laps x 24 cars = 1200 positions.
    let mut lap_positions = padded(header(2026, 26, 15), 1231);
    *lap_positions.last_mut().unwrap() = 7; // position_for_vehicle_idx[1199]
    let packet = parse_packet(&lap_positions).unwrap();
    let value = serde_json::to_value(&packet).unwrap();
    let positions = &value["F1_26"]["LapPositions"]["position_for_vehicle_idx"];
    assert_eq!(positions.as_array().unwrap().len(), 1200);
    assert_eq!(positions[1199], 7);
    round_trip(&packet);

    // F1 24 SessionHistory: 100-lap history array.
    let mut history = header(2024, 24, 11);
    history.extend_from_slice(&[3, 1, 0, 1, 1, 1, 1]);
    history.extend_from_slice(&90_500u32.to_le_bytes());
    round_trip(&parse_packet(&padded(history, 1460)).unwrap());
}

#[test]
fn events_use_external_tagging() {
    let collision = serde_json::to_value(event(2026, 26, b"COLL", &[3, 4, 2])).unwrap();
    assert_eq!(
        collision["F1_26"]["Event"]["event_details"],
        json!({ "Collision": { "vehicle1_idx": 3, "vehicle2_idx": 4, "severity": 2 } })
    );

    let started = serde_json::to_value(event(2025, 25, b"SSTA", &[])).unwrap();
    assert_eq!(started["F1_25"]["Event"]["event_details"], "SessionStarted");

    let payload: Vec<u8> = (1..=12).collect();
    let unknown = event(2023, 23, b"ZZZZ", &payload);
    let value = serde_json::to_value(&unknown).unwrap();
    assert_eq!(
        value["F1_23"]["Event"]["event_details"]["Unknown"]["payload"],
        json!(payload)
    );
    round_trip(&unknown);
}

#[test]
fn common_event_details_round_trip() {
    let Packet::F1_24(v2024::Packet::Event(e)) = event(2024, 24, b"COLL", &[3, 4]) else {
        panic!("expected F1 24 event");
    };
    let details = common::EventDetails::from(&e.event_details);
    let value = serde_json::to_value(details).unwrap();
    assert_eq!(
        value,
        json!({ "Collision": { "vehicle1_idx": 3, "vehicle2_idx": 4, "severity": null } })
    );
    let back: common::EventDetails = serde_json::from_value(value).unwrap();
    assert_eq!(back, details);
}

#[test]
fn version_enums_serialize_as_names() {
    assert_eq!(serde_json::to_value(GameVersion::F1_26).unwrap(), "F1_26");
    assert_eq!(
        serde_json::to_value(PacketKind::CarTelemetry2).unwrap(),
        "CarTelemetry2"
    );
    assert_eq!(
        serde_json::from_str::<GameVersion>("\"F1_26\"").unwrap(),
        GameVersion::F1_26
    );
    assert_eq!(
        serde_json::from_str::<PacketKind>("\"CarTelemetry2\"").unwrap(),
        PacketKind::CarTelemetry2
    );
}

#[test]
fn wrong_array_length_is_an_error() {
    let packet = parse_packet(&padded(header(2023, 23, 0), 1349)).unwrap();
    let mut value = serde_json::to_value(&packet).unwrap();
    let cars = value["F1_23"]["Motion"]["car_motion_data"]
        .as_array_mut()
        .unwrap();
    assert_eq!(cars.len(), 22);
    cars.pop();

    let err = serde_json::from_value::<Packet>(value).unwrap_err();
    assert!(err.to_string().contains("invalid length"), "{err}");
}

#[test]
fn header_and_errors_serialize() {
    let packet = parse_packet(&padded(header(2025, 25, 0), 1349)).unwrap();
    let hdr: Value = serde_json::to_value(packet.header()).unwrap();
    assert_eq!(hdr["packet_format"], 2025);
    assert_eq!(hdr["frame_identifier"], 42);

    let err = parse_packet(&[0u8; 10]).unwrap_err();
    assert_eq!(
        serde_json::to_value(&err).unwrap(),
        json!({ "BufferTooShort": { "needed": 29, "available": 10 } })
    );
}
