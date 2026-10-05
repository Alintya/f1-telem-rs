use f1_telem::*;

// ---------------------------------------------------------------------------
// Helpers for building synthetic binary packets
// ---------------------------------------------------------------------------

struct PacketBuilder {
    buf: Vec<u8>,
}

#[allow(dead_code)]
impl PacketBuilder {
    fn new() -> Self {
        Self {
            buf: Vec::with_capacity(2048),
        }
    }

    fn header(mut self, format: u16, year: u8, packet_id: u8) -> Self {
        self.buf.extend_from_slice(&format.to_le_bytes()); // 2
        self.buf.push(year); // 3
        self.buf.push(1); // major version
        self.buf.push(0); // minor version
        self.buf.push(1); // packet version
        self.buf.push(packet_id); // 7
        self.buf.extend_from_slice(&0u64.to_le_bytes()); // session uid (8)
        self.buf.extend_from_slice(&1.5f32.to_le_bytes()); // session time (4)
        self.buf.extend_from_slice(&42u32.to_le_bytes()); // frame id (4)
        self.buf.extend_from_slice(&100u32.to_le_bytes()); // overall frame id (4)
        self.buf.push(0); // player car index
        self.buf.push(255); // secondary player car index
        assert_eq!(self.buf.len(), 29);
        self
    }

    fn u8(mut self, v: u8) -> Self {
        self.buf.push(v);
        self
    }

    fn i8(mut self, v: i8) -> Self {
        self.buf.push(v as u8);
        self
    }

    fn u16(mut self, v: u16) -> Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    fn u32(mut self, v: u32) -> Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    fn u64(mut self, v: u64) -> Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    fn f32(mut self, v: f32) -> Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    fn f64(mut self, v: f64) -> Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    fn i16(mut self, v: i16) -> Self {
        self.buf.extend_from_slice(&v.to_le_bytes());
        self
    }

    fn zeros(mut self, n: usize) -> Self {
        self.buf.extend(std::iter::repeat_n(0u8, n));
        self
    }

    fn string(mut self, s: &str, len: usize) -> Self {
        let bytes = s.as_bytes();
        let copy_len = bytes.len().min(len);
        self.buf.extend_from_slice(&bytes[..copy_len]);
        self.buf.extend(std::iter::repeat_n(0u8, len - copy_len));
        self
    }

    fn pad_to(mut self, total_size: usize) -> Self {
        if self.buf.len() < total_size {
            self.buf
                .extend(std::iter::repeat_n(0u8, total_size - self.buf.len()));
        }
        self
    }

    fn build(self) -> Vec<u8> {
        self.buf
    }
}

// ---------------------------------------------------------------------------
// F1 23 tests
// ---------------------------------------------------------------------------

#[test]
fn f1_23_motion_packet() {
    let mut b = PacketBuilder::new().header(2023, 23, 0);
    // 22 cars × 60 bytes each = 1320 bytes
    // First car: set specific values
    b = b
        .f32(100.0) // world_position_x
        .f32(200.0) // world_position_y
        .f32(300.0) // world_position_z
        .f32(50.0) // world_velocity_x
        .f32(0.0) // world_velocity_y
        .f32(-10.0) // world_velocity_z
        .i16(16384) // world_forward_dir_x
        .i16(0) // world_forward_dir_y
        .i16(-16384) // world_forward_dir_z
        .i16(0) // world_right_dir_x
        .i16(16384) // world_right_dir_y
        .i16(0) // world_right_dir_z
        .f32(1.5) // g_force_lateral
        .f32(0.3) // g_force_longitudinal
        .f32(1.0) // g_force_vertical
        .f32(0.1) // yaw
        .f32(0.02) // pitch
        .f32(-0.01); // roll
                     // Remaining 21 cars are zeros
    b = b.zeros(21 * 60);
    let data = b.pad_to(1349).build();
    assert_eq!(data.len(), 1349);

    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.version(), GameVersion::F1_23);
    assert_eq!(packet.kind(), PacketKind::Motion);
    assert_eq!(packet.header().packet_format, 2023);
    assert_eq!(packet.header().frame_identifier, 42);
    assert!((packet.header().session_time - 1.5).abs() < 0.001);

    if let Packet::F1_23(v2023::Packet::Motion(m)) = &packet {
        assert_eq!(m.car_motion_data.len(), 22);
        let car0 = &m.car_motion_data[0];
        assert!((car0.world_position_x - 100.0).abs() < 0.001);
        assert!((car0.world_position_z - 300.0).abs() < 0.001);
        assert_eq!(car0.world_forward_dir_x, 16384);
        assert!((car0.g_force_lateral - 1.5).abs() < 0.001);
    } else {
        panic!("Expected F1_23 Motion packet");
    }
}

#[test]
fn f1_23_session_packet() {
    let data = PacketBuilder::new().header(2023, 23, 1).pad_to(644).build();
    assert_eq!(data.len(), 644);

    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.kind(), PacketKind::Session);
    if let Packet::F1_23(v2023::Packet::Session(s)) = &packet {
        assert_eq!(s.weather_forecast_samples.len(), 56);
        assert_eq!(s.marshal_zones.len(), 21);
    } else {
        panic!("Expected F1_23 Session packet");
    }
}

#[test]
fn f1_23_lap_data_packet() {
    let data = PacketBuilder::new()
        .header(2023, 23, 2)
        .pad_to(1131)
        .build();
    assert_eq!(data.len(), 1131);

    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.kind(), PacketKind::LapData);
    if let Packet::F1_23(v2023::Packet::LapData(l)) = &packet {
        assert_eq!(l.lap_data.len(), 22);
    } else {
        panic!("Expected F1_23 LapData packet");
    }
}

#[test]
fn f1_23_event_fastest_lap() {
    let data = PacketBuilder::new()
        .header(2023, 23, 3)
        .u8(b'F')
        .u8(b'T')
        .u8(b'L')
        .u8(b'P') // event code
        .u8(5) // vehicle_idx
        .f32(78.5) // lap_time
        .pad_to(45)
        .build();
    assert_eq!(data.len(), 45);

    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.kind(), PacketKind::Event);
    if let Packet::F1_23(v2023::Packet::Event(e)) = &packet {
        assert_eq!(&e.event_code, b"FTLP");
        if let v2023::EventDetails::FastestLap {
            vehicle_idx,
            lap_time,
        } = &e.event_details
        {
            assert_eq!(*vehicle_idx, 5);
            assert!((lap_time - 78.5).abs() < 0.001);
        } else {
            panic!("Expected FastestLap event details");
        }
    } else {
        panic!("Expected F1_23 Event packet");
    }
}

#[test]
fn f1_23_event_session_started() {
    let data = PacketBuilder::new()
        .header(2023, 23, 3)
        .u8(b'S')
        .u8(b'S')
        .u8(b'T')
        .u8(b'A')
        .pad_to(45)
        .build();

    let packet = parse_packet(&data).unwrap();
    if let Packet::F1_23(v2023::Packet::Event(e)) = &packet {
        assert!(matches!(
            e.event_details,
            v2023::EventDetails::SessionStarted
        ));
    } else {
        panic!("Expected F1_23 Event packet");
    }
}

#[test]
fn f1_23_event_penalty() {
    let data = PacketBuilder::new()
        .header(2023, 23, 3)
        .u8(b'P')
        .u8(b'E')
        .u8(b'N')
        .u8(b'A')
        .u8(2) // penalty_type
        .u8(5) // infringement_type
        .u8(7) // vehicle_idx
        .u8(12) // other_vehicle_idx
        .u8(3) // time
        .u8(15) // lap_num
        .u8(1) // places_gained
        .pad_to(45)
        .build();

    let packet = parse_packet(&data).unwrap();
    if let Packet::F1_23(v2023::Packet::Event(e)) = &packet {
        if let v2023::EventDetails::Penalty {
            penalty_type,
            vehicle_idx,
            lap_num,
            ..
        } = &e.event_details
        {
            assert_eq!(*penalty_type, 2);
            assert_eq!(*vehicle_idx, 7);
            assert_eq!(*lap_num, 15);
        } else {
            panic!("Expected Penalty event");
        }
    } else {
        panic!("Expected F1_23 Event packet");
    }
}

#[test]
fn f1_23_participants_packet() {
    let mut b = PacketBuilder::new().header(2023, 23, 4).u8(20); // num_active_cars

    // First participant with real data
    b = b
        .u8(0) // ai_controlled (human)
        .u8(7) // driver_id (Hamilton)
        .u8(0) // network_id
        .u8(0) // team_id (Mercedes)
        .u8(0) // my_team
        .u8(44) // race_number
        .u8(10) // nationality (British)
        .string("Lewis Hamilton", 48)
        .u8(1) // your_telemetry (public)
        .u8(1) // show_online_names
        .u8(1); // platform (Steam)

    // Remaining 21 participants as zeros
    b = b.zeros(21 * 58);
    let data = b.pad_to(1306).build();
    assert_eq!(data.len(), 1306);

    let packet = parse_packet(&data).unwrap();
    if let Packet::F1_23(v2023::Packet::Participants(p)) = &packet {
        assert_eq!(p.num_active_cars, 20);
        assert_eq!(p.participants.len(), 22);
        assert_eq!(p.participants[0].name, "Lewis Hamilton");
        assert_eq!(p.participants[0].race_number, 44);
        assert_eq!(p.participants[0].driver_id, 7);
    } else {
        panic!("Expected F1_23 Participants packet");
    }
}

#[test]
fn f1_23_participants_invalid_utf8_name_still_parses() {
    let data = PacketBuilder::new()
        .header(2023, 23, 4)
        .u8(1) // num_active_cars
        .zeros(7) // ai_controlled .. nationality
        .u8(0xFF) // invalid UTF-8 lead byte
        .u8(b'a')
        .zeros(46) // rest of the 48-byte name
        .pad_to(1306)
        .build();

    let packet = parse_packet(&data).unwrap();
    if let Packet::F1_23(v2023::Packet::Participants(p)) = &packet {
        let name = &p.participants[0].name;
        assert!(name.as_str().is_err());
        assert_eq!(name.to_string_lossy(), "\u{FFFD}a");
    } else {
        panic!("Expected F1_23 Participants packet");
    }
}

#[test]
fn f1_23_car_setups_packet() {
    let data = PacketBuilder::new()
        .header(2023, 23, 5)
        .pad_to(1107)
        .build();
    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.kind(), PacketKind::CarSetups);
}

#[test]
fn f1_23_car_telemetry_packet() {
    let data = PacketBuilder::new()
        .header(2023, 23, 6)
        .pad_to(1352)
        .build();
    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.kind(), PacketKind::CarTelemetry);
}

#[test]
fn f1_23_car_status_packet() {
    let data = PacketBuilder::new()
        .header(2023, 23, 7)
        .pad_to(1239)
        .build();
    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.kind(), PacketKind::CarStatus);
}

#[test]
fn f1_23_final_classification_packet() {
    let data = PacketBuilder::new()
        .header(2023, 23, 8)
        .pad_to(1020)
        .build();
    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.kind(), PacketKind::FinalClassification);
}

#[test]
fn f1_23_lobby_info_packet() {
    let data = PacketBuilder::new()
        .header(2023, 23, 9)
        .pad_to(1218)
        .build();
    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.kind(), PacketKind::LobbyInfo);
}

#[test]
fn f1_23_car_damage_packet() {
    let data = PacketBuilder::new()
        .header(2023, 23, 10)
        .pad_to(953)
        .build();
    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.kind(), PacketKind::CarDamage);
}

#[test]
fn f1_23_session_history_packet() {
    let data = PacketBuilder::new()
        .header(2023, 23, 11)
        .pad_to(1460)
        .build();
    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.kind(), PacketKind::SessionHistory);
}

#[test]
fn f1_23_tyre_sets_packet() {
    let data = PacketBuilder::new()
        .header(2023, 23, 12)
        .pad_to(231)
        .build();
    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.kind(), PacketKind::TyreSets);
}

#[test]
fn f1_23_motion_ex_packet() {
    let data = PacketBuilder::new()
        .header(2023, 23, 13)
        .pad_to(217)
        .build();
    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.kind(), PacketKind::MotionEx);
}

// ---------------------------------------------------------------------------
// F1 24 tests
// ---------------------------------------------------------------------------

#[test]
fn f1_24_session_packet_with_new_fields() {
    let data = PacketBuilder::new().header(2024, 24, 1).pad_to(753).build();
    assert_eq!(data.len(), 753);

    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.version(), GameVersion::F1_24);
    assert_eq!(packet.kind(), PacketKind::Session);
    if let Packet::F1_24(v2024::Packet::Session(s)) = &packet {
        // F1 24 has 64 weather samples (up from 56)
        assert_eq!(s.weather_forecast_samples.len(), 64);
    } else {
        panic!("Expected F1_24 Session packet");
    }
}

#[test]
fn f1_24_lap_data_with_speed_trap() {
    let data = PacketBuilder::new()
        .header(2024, 24, 2)
        .pad_to(1285)
        .build();
    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.kind(), PacketKind::LapData);
    if let Packet::F1_24(v2024::Packet::LapData(l)) = &packet {
        assert_eq!(l.lap_data.len(), 22);
    } else {
        panic!("Expected F1_24 LapData packet");
    }
}

#[test]
fn f1_24_participants_with_tech_level() {
    let mut b = PacketBuilder::new().header(2024, 24, 4).u8(1); // num_active_cars

    // Participant with tech level
    b = b
        .u8(0) // ai_controlled
        .u8(9) // driver_id (Verstappen)
        .u8(0) // network_id
        .u8(2) // team_id (Red Bull)
        .u8(0) // my_team
        .u8(1) // race_number
        .u8(4) // nationality (Austrian..ish, actually Dutch=46? Doesn't matter)
        .string("Max Verstappen", 48)
        .u8(1) // your_telemetry
        .u8(1) // show_online_names
        .u16(500) // tech_level
        .u8(1); // platform

    b = b.zeros(21 * 60); // rest of participants
    let data = b.pad_to(1350).build();
    assert_eq!(data.len(), 1350);

    let packet = parse_packet(&data).unwrap();
    if let Packet::F1_24(v2024::Packet::Participants(p)) = &packet {
        assert_eq!(p.participants[0].name, "Max Verstappen");
        assert_eq!(p.participants[0].tech_level, 500);
    } else {
        panic!("Expected F1_24 Participants packet");
    }
}

#[test]
fn f1_24_car_setups_with_engine_braking() {
    let data = PacketBuilder::new()
        .header(2024, 24, 5)
        .pad_to(1133)
        .build();
    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.kind(), PacketKind::CarSetups);
    if let Packet::F1_24(v2024::Packet::CarSetups(s)) = &packet {
        assert_eq!(s.car_setups.len(), 22);
    } else {
        panic!("Expected F1_24 CarSetups packet");
    }
}

#[test]
fn f1_24_motion_ex_with_aero_heights() {
    let data = PacketBuilder::new()
        .header(2024, 24, 13)
        .pad_to(237)
        .build();
    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.kind(), PacketKind::MotionEx);
}

#[test]
fn f1_24_time_trial_packet() {
    let data = PacketBuilder::new()
        .header(2024, 24, 14)
        .pad_to(101)
        .build();
    assert_eq!(data.len(), 101);

    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.version(), GameVersion::F1_24);
    assert_eq!(packet.kind(), PacketKind::TimeTrial);
}

#[test]
fn f1_24_event_safety_car() {
    let data = PacketBuilder::new()
        .header(2024, 24, 3)
        .u8(b'S')
        .u8(b'C')
        .u8(b'A')
        .u8(b'R')
        .u8(1) // safety_car_type = Full
        .u8(0) // event_type = Deployed
        .pad_to(45)
        .build();

    let packet = parse_packet(&data).unwrap();
    if let Packet::F1_24(v2024::Packet::Event(e)) = &packet {
        if let v2024::EventDetails::SafetyCar {
            safety_car_type,
            event_type,
        } = &e.event_details
        {
            assert_eq!(*safety_car_type, 1);
            assert_eq!(*event_type, 0);
        } else {
            panic!("Expected SafetyCar event");
        }
    } else {
        panic!("Expected F1_24 Event packet");
    }
}

#[test]
fn f1_24_event_collision() {
    let data = PacketBuilder::new()
        .header(2024, 24, 3)
        .u8(b'C')
        .u8(b'O')
        .u8(b'L')
        .u8(b'L')
        .u8(3) // vehicle1_idx
        .u8(17) // vehicle2_idx
        .pad_to(45)
        .build();

    let packet = parse_packet(&data).unwrap();
    if let Packet::F1_24(v2024::Packet::Event(e)) = &packet {
        if let v2024::EventDetails::Collision {
            vehicle1_idx,
            vehicle2_idx,
        } = &e.event_details
        {
            assert_eq!(*vehicle1_idx, 3);
            assert_eq!(*vehicle2_idx, 17);
        } else {
            panic!("Expected Collision event");
        }
    } else {
        panic!("Expected F1_24 Event packet");
    }
}

#[test]
fn f1_24_lobby_info_packet() {
    let data = PacketBuilder::new()
        .header(2024, 24, 9)
        .pad_to(1306)
        .build();
    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.kind(), PacketKind::LobbyInfo);
}

// ---------------------------------------------------------------------------
// F1 25 tests
// ---------------------------------------------------------------------------

#[test]
fn f1_25_participants_with_livery() {
    let mut b = PacketBuilder::new().header(2025, 25, 4).u8(1); // num_active_cars

    // ParticipantData for F1 25: name is 32 bytes, has livery colours
    b = b
        .u8(0) // ai_controlled
        .u8(58) // driver_id (Leclerc)
        .u8(0) // network_id
        .u8(1) // team_id (Ferrari)
        .u8(0) // my_team
        .u8(16) // race_number
        .u8(41) // nationality
        .string("Charles Leclerc", 32)
        .u8(1) // your_telemetry
        .u8(1) // show_online_names
        .u16(750) // tech_level
        .u8(1) // platform
        .u8(2) // num_colours
        // livery_colours[4] = 4 × 3 bytes = 12 bytes
        .u8(255)
        .u8(0)
        .u8(0) // red
        .u8(255)
        .u8(255)
        .u8(0) // yellow
        .u8(0)
        .u8(0)
        .u8(0) // unused
        .u8(0)
        .u8(0)
        .u8(0); // unused

    b = b.zeros(21 * 57); // remaining participants
    let data = b.pad_to(1284).build();
    assert_eq!(data.len(), 1284);

    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.version(), GameVersion::F1_25);
    if let Packet::F1_25(v2025::Packet::Participants(p)) = &packet {
        let part = &p.participants[0];
        assert_eq!(part.name, "Charles Leclerc");
        assert_eq!(part.team_id, 1);
        assert_eq!(part.num_colours, 2);
        assert_eq!(part.livery_colours.len(), 4);
        assert_eq!(part.livery_colours[0].red, 255);
        assert_eq!(part.livery_colours[0].green, 0);
        assert_eq!(part.livery_colours[1].green, 255);
    } else {
        panic!("Expected F1_25 Participants packet");
    }
}

#[test]
fn f1_25_final_classification_with_result_reason() {
    let mut b = PacketBuilder::new().header(2025, 25, 8).u8(1); // num_cars

    // FinalClassificationData for F1 25: 46 bytes (adds result_reason)
    b = b
        .u8(1) // position
        .u8(57) // num_laps
        .u8(3) // grid_position
        .u8(25) // points
        .u8(2) // num_pit_stops
        .u8(3) // result_status = finished
        .u8(2) // result_reason = finished
        .u32(72000) // best_lap_time_in_ms
        .f64(5400.5) // total_race_time
        .u8(0) // penalties_time
        .u8(0) // num_penalties
        .u8(3) // num_tyre_stints
        .zeros(24); // tyre stints arrays (3 × 8 bytes)

    b = b.zeros(21 * 46); // remaining cars
    let data = b.pad_to(1042).build();
    assert_eq!(data.len(), 1042);

    let packet = parse_packet(&data).unwrap();
    if let Packet::F1_25(v2025::Packet::FinalClassification(fc)) = &packet {
        assert_eq!(fc.num_cars, 1);
        let c = &fc.classification_data[0];
        assert_eq!(c.position, 1);
        assert_eq!(c.points, 25);
        assert_eq!(c.result_reason, 2);
        assert_eq!(c.best_lap_time_in_ms, 72000);
    } else {
        panic!("Expected F1_25 FinalClassification packet");
    }
}

#[test]
fn f1_25_car_damage_with_blisters() {
    let data = PacketBuilder::new()
        .header(2025, 25, 10)
        .pad_to(1041)
        .build();
    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.kind(), PacketKind::CarDamage);
    if let Packet::F1_25(v2025::Packet::CarDamage(d)) = &packet {
        assert_eq!(d.car_damage_data.len(), 22);
    } else {
        panic!("Expected F1_25 CarDamage packet");
    }
}

#[test]
fn f1_25_motion_ex_with_camber() {
    let data = PacketBuilder::new()
        .header(2025, 25, 13)
        .pad_to(273)
        .build();
    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.kind(), PacketKind::MotionEx);
}

#[test]
fn f1_25_lap_positions_packet() {
    let mut b = PacketBuilder::new()
        .header(2025, 25, 15)
        .u8(5) // num_laps
        .u8(0); // lap_start
                // position_for_vehicle_idx: 1100 bytes (50 laps × 22 cars)
    b = b.zeros(1100);
    let data = b.pad_to(1131).build();
    assert_eq!(data.len(), 1131);

    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.version(), GameVersion::F1_25);
    assert_eq!(packet.kind(), PacketKind::LapPositions);
    if let Packet::F1_25(v2025::Packet::LapPositions(lp)) = &packet {
        assert_eq!(lp.num_laps, 5);
        assert_eq!(lp.lap_start, 0);
        assert_eq!(lp.position_for_vehicle_idx.len(), 1100);
    } else {
        panic!("Expected F1_25 LapPositions packet");
    }
}

#[test]
fn f1_25_lobby_info_short_name() {
    let data = PacketBuilder::new().header(2025, 25, 9).pad_to(954).build();
    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.kind(), PacketKind::LobbyInfo);
    if let Packet::F1_25(v2025::Packet::LobbyInfo(l)) = &packet {
        assert_eq!(l.lobby_players.len(), 22);
    } else {
        panic!("Expected F1_25 LobbyInfo packet");
    }
}

#[test]
fn f1_25_event_retirement_with_reason() {
    let data = PacketBuilder::new()
        .header(2025, 25, 3)
        .u8(b'R')
        .u8(b'T')
        .u8(b'M')
        .u8(b'T')
        .u8(14) // vehicle_idx
        .u8(3) // reason = terminal damage
        .pad_to(45)
        .build();

    let packet = parse_packet(&data).unwrap();
    if let Packet::F1_25(v2025::Packet::Event(e)) = &packet {
        if let v2025::EventDetails::Retirement {
            vehicle_idx,
            reason,
        } = &e.event_details
        {
            assert_eq!(*vehicle_idx, 14);
            assert_eq!(*reason, 3);
        } else {
            panic!("Expected Retirement event");
        }
    } else {
        panic!("Expected F1_25 Event packet");
    }
}

#[test]
fn f1_25_event_drs_disabled_with_reason() {
    let data = PacketBuilder::new()
        .header(2025, 25, 3)
        .u8(b'D')
        .u8(b'R')
        .u8(b'S')
        .u8(b'D')
        .u8(1) // reason = Safety car deployed
        .pad_to(45)
        .build();

    let packet = parse_packet(&data).unwrap();
    if let Packet::F1_25(v2025::Packet::Event(e)) = &packet {
        if let v2025::EventDetails::DrsDisabled { reason } = &e.event_details {
            assert_eq!(*reason, 1);
        } else {
            panic!("Expected DrsDisabled event");
        }
    } else {
        panic!("Expected F1_25 Event packet");
    }
}

#[test]
fn f1_25_event_stop_go_with_stop_time() {
    let data = PacketBuilder::new()
        .header(2025, 25, 3)
        .u8(b'S')
        .u8(b'G')
        .u8(b'S')
        .u8(b'V')
        .u8(7) // vehicle_idx
        .f32(10.5) // stop_time
        .pad_to(45)
        .build();

    let packet = parse_packet(&data).unwrap();
    if let Packet::F1_25(v2025::Packet::Event(e)) = &packet {
        if let v2025::EventDetails::StopGoServed {
            vehicle_idx,
            stop_time,
        } = &e.event_details
        {
            assert_eq!(*vehicle_idx, 7);
            assert!((stop_time - 10.5).abs() < 0.001);
        } else {
            panic!("Expected StopGoServed event");
        }
    } else {
        panic!("Expected F1_25 Event packet");
    }
}

#[test]
fn f1_25_event_unknown_code_keeps_payload() {
    let mut b = PacketBuilder::new().header(2025, 25, 3);
    for c in *b"ZZZZ" {
        b = b.u8(c);
    }
    for v in 1..=12u8 {
        b = b.u8(v);
    }
    let data = b.build();
    assert_eq!(data.len(), 45);

    let packet = parse_packet(&data).unwrap();
    if let Packet::F1_25(v2025::Packet::Event(e)) = &packet {
        assert_eq!(
            e.event_details,
            v2025::EventDetails::Unknown {
                code: *b"ZZZZ",
                payload: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
            }
        );
    } else {
        panic!("Expected F1_25 Event packet");
    }
}

// ---------------------------------------------------------------------------
// F1 25 2026 Season Pack (packet format 2026)
// ---------------------------------------------------------------------------

#[test]
fn f1_26_motion_with_quantised_g_force() {
    let data = PacketBuilder::new()
        .header(2026, 26, 0)
        .f32(10.0) // world_position_x
        .zeros(5 * 4 + 6 * 2) // rest of position/velocity, direction vectors
        .i16(1500) // g_force_lateral (1.5 g)
        .i16(-250) // g_force_longitudinal
        .i16(1000) // g_force_vertical
        .f32(0.5) // yaw
        .pad_to(1325)
        .build();

    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.version(), GameVersion::F1_26);
    if let Packet::F1_26(v2026::Packet::Motion(m)) = &packet {
        assert_eq!(m.car_motion_data.len(), 24);
        let car = &m.car_motion_data[0];
        assert!((car.world_position_x - 10.0).abs() < 0.001);
        assert_eq!(car.g_force_lateral, 1500);
        assert_eq!(car.g_force_longitudinal, -250);
        assert_eq!(car.g_force_vertical, 1000);
        assert!((car.yaw - 0.5).abs() < 0.001);
    } else {
        panic!("Expected F1_26 Motion packet");
    }
}

#[test]
fn f1_26_session_with_aero_and_drs_zones() {
    let data = PacketBuilder::new()
        .header(2026, 26, 1)
        .zeros(724) // F1 25 session fields
        .u8(1) // active_aero_track_status (partial)
        .u8(2) // num_active_aero_zones_full
        .f32(0.1) // active_aero_zones_full[0].zone_start
        .f32(0.2) // active_aero_zones_full[0].zone_end
        .zeros(7 * 8)
        .u8(0) // num_active_aero_zones_partial
        .zeros(8 * 8)
        .u8(1) // num_drs_zones
        .f32(0.5) // drs_zones[0].zone_start
        .f32(0.6) // drs_zones[0].zone_end
        .zeros(3 * 8)
        .f32(0.215) // start_reaction_time
        .u8(1) // anti_lock_brakes_assist
        .u8(2) // traction_control_assist
        .u8(1) // dynamic_racing_line_hi_vis
        .u8(3) // dynamic_racing_line_colour_blind
        .u8(1) // recurring_rewind_prompt
        .build();
    assert_eq!(data.len(), 926);

    let packet = parse_packet(&data).unwrap();
    if let Packet::F1_26(v2026::Packet::Session(s)) = &packet {
        assert_eq!(s.active_aero_track_status, 1);
        assert_eq!(s.num_active_aero_zones_full, 2);
        assert!((s.active_aero_zones_full[0].zone_end - 0.2).abs() < 0.001);
        assert_eq!(s.num_drs_zones, 1);
        assert!((s.drs_zones[0].zone_start - 0.5).abs() < 0.001);
        assert!((s.start_reaction_time - 0.215).abs() < 0.001);
        assert_eq!(s.dynamic_racing_line_colour_blind, 3);
        assert_eq!(s.recurring_rewind_prompt, 1);
    } else {
        panic!("Expected F1_26 Session packet");
    }
}

#[test]
fn f1_26_participants_with_u16_ids() {
    let data = PacketBuilder::new()
        .header(2026, 26, 4)
        .u8(22) // num_active_cars
        .u8(0) // ai_controlled
        .u16(65535) // driver_id (network human)
        .u16(7) // network_id
        .u16(489) // team_id (F2 '26 team, > 255)
        .u8(0) // my_team
        .u8(1) // race_number
        .u8(10) // nationality
        .string("Max", 32)
        .pad_to(1470)
        .build();

    let packet = parse_packet(&data).unwrap();
    if let Packet::F1_26(v2026::Packet::Participants(p)) = &packet {
        assert_eq!(p.participants.len(), 24);
        let part = &p.participants[0];
        assert_eq!(part.driver_id, 65535);
        assert_eq!(part.network_id, 7);
        assert_eq!(part.team_id, 489);
        assert_eq!(part.race_number, 1);
        assert_eq!(part.name, "Max");
    } else {
        panic!("Expected F1_26 Participants packet");
    }
}

#[test]
fn f1_26_car_telemetry_with_u8_engine_temperature() {
    let data = PacketBuilder::new()
        .header(2026, 26, 6)
        .u16(300) // speed
        .zeros(36) // throttle .. tyres_inner_temperature
        .u8(105) // engine_temperature (u8 in 2026, u16 before)
        .f32(23.5) // tyres_pressure[0]
        .pad_to(1448)
        .build();

    let packet = parse_packet(&data).unwrap();
    if let Packet::F1_26(v2026::Packet::CarTelemetry(t)) = &packet {
        let car = &t.car_telemetry_data[0];
        assert_eq!(t.car_telemetry_data.len(), 24);
        assert_eq!(car.speed, 300);
        assert_eq!(car.engine_temperature, 105u8);
        assert!((car.tyres_pressure[0] - 23.5).abs() < 0.001);
        // The common view widens it to the u16 earlier versions send.
        assert_eq!(common::CarTelemetryData::engine_temperature(car), 105u16);
    } else {
        panic!("Expected F1_26 CarTelemetry packet");
    }
}

#[test]
fn f1_26_car_status_with_harvest_limit() {
    let data = PacketBuilder::new()
        .header(2026, 26, 7)
        .zeros(50) // CarStatusData fields up to ers_harvested_this_lap_mguh
        .f32(4_000_000.0) // ers_harvest_limit_per_lap
        .f32(1_500_000.0) // ers_deployed_this_lap
        .u8(1) // network_paused
        .pad_to(1445)
        .build();

    let packet = parse_packet(&data).unwrap();
    if let Packet::F1_26(v2026::Packet::CarStatus(s)) = &packet {
        assert_eq!(s.car_status_data.len(), 24);
        let car = &s.car_status_data[0];
        assert!((car.ers_harvest_limit_per_lap - 4_000_000.0).abs() < 1.0);
        assert!((car.ers_deployed_this_lap - 1_500_000.0).abs() < 1.0);
        assert_eq!(car.network_paused, 1);
    } else {
        panic!("Expected F1_26 CarStatus packet");
    }
}

#[test]
fn f1_26_car_telemetry2_packet() {
    let data = PacketBuilder::new()
        .header(2026, 26, 16)
        .zeros(10) // car 0
        .u8(1) // car 1: active_aero_mode (straight)
        .u8(1) // active_aero_available
        .u16(150) // active_aero_activation_distance
        .u8(0) // overtake_available
        .u8(1) // overtake_active
        .u16(0) // overtake_activation_distance
        .u8(1) // regulations_2026
        .u8(0) // driving_wrong_way
        .pad_to(269)
        .build();

    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.kind(), PacketKind::CarTelemetry2);
    if let Packet::F1_26(v2026::Packet::CarTelemetry2(t)) = &packet {
        assert_eq!(t.car_telemetry2_data.len(), 24);
        let car = &t.car_telemetry2_data[1];
        assert_eq!(car.active_aero_mode, 1);
        assert_eq!(car.active_aero_activation_distance, 150);
        assert_eq!(car.overtake_active, 1);
        assert_eq!(car.regulations_2026, 1);
    } else {
        panic!("Expected F1_26 CarTelemetry2 packet");
    }
}

#[test]
fn f1_26_event_collision_with_severity() {
    let data = PacketBuilder::new()
        .header(2026, 26, 3)
        .string("COLL", 4)
        .u8(3) // vehicle1_idx
        .u8(23) // vehicle2_idx (24-car grid)
        .u8(2) // severity (high)
        .pad_to(45)
        .build();

    let packet = parse_packet(&data).unwrap();
    if let Packet::F1_26(v2026::Packet::Event(e)) = &packet {
        assert_eq!(
            e.event_details,
            v2026::EventDetails::Collision {
                vehicle1_idx: 3,
                vehicle2_idx: 23,
                severity: 2,
            }
        );
    } else {
        panic!("Expected F1_26 Event packet");
    }
}

#[test]
fn f1_26_event_partial_mode_enabled() {
    let data = PacketBuilder::new()
        .header(2026, 26, 3)
        .string("PMEN", 4)
        .u8(1) // reason: safety car deployed
        .pad_to(45)
        .build();

    let packet = parse_packet(&data).unwrap();
    if let Packet::F1_26(v2026::Packet::Event(e)) = &packet {
        assert_eq!(
            e.event_details,
            v2026::EventDetails::PartialModeEnabled { reason: 1 }
        );
    } else {
        panic!("Expected F1_26 Event packet");
    }
}

#[test]
fn f1_25_rejects_car_telemetry2() {
    let data = PacketBuilder::new()
        .header(2025, 25, 16)
        .pad_to(269)
        .build();

    assert_eq!(
        parse_packet(&data).unwrap_err(),
        ParseError::UnknownPacketId {
            packet_id: 16,
            format: 2025,
        }
    );
}

// ---------------------------------------------------------------------------
// Error cases
// ---------------------------------------------------------------------------

#[test]
fn unsupported_format() {
    let data = PacketBuilder::new()
        .header(2022, 22, 0)
        .pad_to(1349)
        .build();

    match parse_packet(&data) {
        Err(ParseError::UnsupportedFormat(2022)) => {}
        other => panic!("Expected UnsupportedFormat(2022), got {other:?}"),
    }
}

#[test]
fn unknown_packet_id() {
    let data = PacketBuilder::new()
        .header(2023, 23, 14) // 14 is TimeTrial, only in F1 24+
        .pad_to(200)
        .build();

    match parse_packet(&data) {
        Err(ParseError::UnknownPacketId {
            packet_id: 14,
            format: 2023,
        }) => {}
        other => panic!("Expected UnknownPacketId, got {other:?}"),
    }
}

#[test]
fn unknown_event_code() {
    let data = PacketBuilder::new()
        .header(2023, 23, 3)
        .u8(b'Z')
        .u8(b'Z')
        .u8(b'Z')
        .u8(b'Z')
        .pad_to(45)
        .build();

    let packet = parse_packet(&data).unwrap();
    if let Packet::F1_23(v2023::Packet::Event(e)) = &packet {
        assert_eq!(e.event_code, *b"ZZZZ");
        assert_eq!(
            e.event_details,
            v2023::EventDetails::Unknown {
                code: *b"ZZZZ",
                payload: [0; 12],
            }
        );
    } else {
        panic!("Expected F1_23 Event packet, got {packet:?}");
    }
}

#[test]
fn buffer_too_short_for_header() {
    let data = [0u8; 10];
    match parse_packet(&data) {
        Err(ParseError::BufferTooShort { .. }) => {}
        other => panic!("Expected BufferTooShort, got {other:?}"),
    }
}

#[test]
fn buffer_too_short_for_body() {
    // Valid header but body truncated — caught by runtime size check
    let data = PacketBuilder::new()
        .header(2023, 23, 0)
        .zeros(10) // motion packet needs 1320 more bytes
        .build();

    match parse_packet(&data) {
        Err(ParseError::SizeMismatch { expected: 1349, .. }) => {}
        other => panic!("Expected SizeMismatch for motion packet, got {other:?}"),
    }
}

#[test]
fn oversized_datagram_is_accepted() {
    let data = PacketBuilder::new()
        .header(2023, 23, 0)
        .pad_to(1349)
        .zeros(10)
        .build();

    let packet = parse_packet(&data).unwrap();
    assert_eq!(packet.kind(), PacketKind::Motion);
}

// ---------------------------------------------------------------------------
// Version-independent view (`common`)
// ---------------------------------------------------------------------------

/// Records what the common view exposes, through `Packet::visit` only.
#[derive(Default)]
struct Recorder {
    motion_calls: usize,
    cars: usize,
    g_force_lateral: f32,
    team_id: Option<u16>,
    name: Option<String>,
    has_livery: Option<bool>,
    harvest_limit: Option<Option<f32>>,
    event: Option<common::EventDetails>,
}

impl common::PacketVisitor for Recorder {
    fn motion<P: common::PacketMotion>(&mut self, p: &P) {
        use common::CarMotionData;
        self.motion_calls += 1;
        self.cars = p.car_motion_data().len();
        self.g_force_lateral = p.car_motion_data()[0].g_force_lateral();
    }

    fn participants<P: common::PacketParticipants>(&mut self, p: &P) {
        use common::ParticipantData;
        let first = &p.participants()[0];
        self.team_id = Some(first.team_id());
        self.name = Some(first.name().into_owned());
        self.has_livery = Some(first.livery_colours().is_some());
    }

    fn car_status<P: common::PacketCarStatus>(&mut self, p: &P) {
        use common::CarStatusData;
        self.harvest_limit = Some(p.car_status_data()[0].ers_harvest_limit_per_lap());
    }

    fn event<P: common::PacketEvent>(&mut self, p: &P) {
        self.event = Some(p.event_details());
    }
}

fn record(data: &[u8]) -> Recorder {
    let mut r = Recorder::default();
    parse_packet(data).unwrap().visit(&mut r);
    r
}

#[test]
fn common_g_force_is_physical_across_versions() {
    // F1 25 sends g-force as f32; 2026 sends it as i16 thousandths.
    let f1_25 = PacketBuilder::new()
        .header(2025, 25, 0)
        .zeros(6 * 4 + 6 * 2)
        .f32(1.5)
        .pad_to(1349)
        .build();
    let f1_26 = PacketBuilder::new()
        .header(2026, 26, 0)
        .zeros(6 * 4 + 6 * 2)
        .i16(1500)
        .pad_to(1325)
        .build();

    let (a, b) = (record(&f1_25), record(&f1_26));
    assert!((a.g_force_lateral - 1.5).abs() < 1e-6);
    assert!((b.g_force_lateral - 1.5).abs() < 1e-6);
    assert_eq!((a.cars, b.cars), (22, 24));
}

#[test]
fn common_team_id_widens_and_livery_is_optional() {
    let f1_23 = PacketBuilder::new()
        .header(2023, 23, 4)
        .u8(1) // num_active_cars
        .u8(0) // ai_controlled
        .u8(0) // driver_id
        .u8(0) // network_id
        .u8(255) // team_id (u8)
        .zeros(3) // my_team, race_number, nationality
        .string("Lewis", 48)
        .pad_to(1306)
        .build();
    let f1_26 = PacketBuilder::new()
        .header(2026, 26, 4)
        .u8(1)
        .u8(0)
        .u16(0)
        .u16(0)
        .u16(489) // team_id (u16)
        .zeros(3)
        .string("Max", 32)
        .pad_to(1470)
        .build();

    let (a, b) = (record(&f1_23), record(&f1_26));
    assert_eq!(a.team_id, Some(255));
    assert_eq!(a.name.as_deref(), Some("Lewis"));
    assert_eq!(a.has_livery, Some(false));
    assert_eq!(b.team_id, Some(489));
    assert_eq!(b.name.as_deref(), Some("Max"));
    assert_eq!(b.has_livery, Some(true));
}

#[test]
fn common_field_missing_in_version_is_none() {
    let f1_25 = PacketBuilder::new()
        .header(2025, 25, 7)
        .pad_to(1239)
        .build();
    let f1_26 = PacketBuilder::new()
        .header(2026, 26, 7)
        .zeros(50)
        .f32(2.5) // ers_harvest_limit_per_lap
        .pad_to(1445)
        .build();

    assert_eq!(record(&f1_25).harvest_limit, Some(None));
    assert_eq!(record(&f1_26).harvest_limit, Some(Some(2.5)));
}

#[test]
fn common_event_details_fill_missing_fields_with_none() {
    let event = |format: u16, year: u8, code: &str, payload: &[u8]| {
        let mut b = PacketBuilder::new().header(format, year, 3).string(code, 4);
        for &v in payload {
            b = b.u8(v);
        }
        record(&b.pad_to(45).build()).event.unwrap()
    };

    assert_eq!(
        event(2023, 23, "DRSD", &[]),
        common::EventDetails::DrsDisabled { reason: None }
    );
    assert_eq!(
        event(2025, 25, "DRSD", &[1]),
        common::EventDetails::DrsDisabled { reason: Some(1) }
    );
    assert_eq!(
        event(2024, 24, "COLL", &[3, 4, 2]),
        common::EventDetails::Collision {
            vehicle1_idx: 3,
            vehicle2_idx: 4,
            severity: None,
        }
    );
    assert_eq!(
        event(2026, 26, "COLL", &[3, 4, 2]),
        common::EventDetails::Collision {
            vehicle1_idx: 3,
            vehicle2_idx: 4,
            severity: Some(2),
        }
    );
    // Codes a version does not know stay Unknown, even if a later version defines them.
    assert!(matches!(
        event(2025, 25, "PMEN", &[1]),
        common::EventDetails::Unknown { code, .. } if &code == b"PMEN"
    ));
}

#[test]
fn visitor_calls_only_the_matching_method() {
    let motion = PacketBuilder::new()
        .header(2024, 24, 0)
        .pad_to(1349)
        .build();
    let session = PacketBuilder::new().header(2024, 24, 1).pad_to(753).build();

    assert_eq!(record(&motion).motion_calls, 1);
    let r = record(&session); // Session uses the default no-op method
    assert_eq!(r.motion_calls, 0);
    assert!(r.event.is_none() && r.team_id.is_none());
}

// ---------------------------------------------------------------------------
// Generated version tables
// ---------------------------------------------------------------------------

#[test]
fn packet_kind_id_round_trip() {
    for id in 0..=16u8 {
        assert_eq!(PacketKind::from_id(id).unwrap().id(), id);
    }
    assert!(PacketKind::from_id(17).is_none());
}

#[test]
fn game_version_from_format() {
    for (format, year, version) in [
        (2023, 23, GameVersion::F1_23),
        (2024, 24, GameVersion::F1_24),
        (2025, 25, GameVersion::F1_25),
        (2026, 26, GameVersion::F1_26),
    ] {
        assert_eq!(GameVersion::from_format(format), Some(version));
        assert_eq!(version.format(), format);
        assert_eq!(version.year(), year);
    }
    assert_eq!(GameVersion::F1_25.to_string(), "F1 25");
    assert!(GameVersion::from_format(2022).is_none());
}
