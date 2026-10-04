use std::net::UdpSocket;

use f1_telem::{parse_packet, Packet};

fn main() {
    let addr = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "0.0.0.0:20777".into());

    let socket = UdpSocket::bind(&addr).unwrap_or_else(|e| {
        eprintln!("failed to bind {addr}: {e}");
        std::process::exit(1);
    });
    eprintln!("listening on {addr}");

    let mut buf = [0u8; 2048];
    loop {
        let (len, src) = match socket.recv_from(&mut buf) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("recv error: {e}");
                continue;
            }
        };

        match parse_packet(&buf[..len]) {
            Ok(packet) => print_packet(&packet, src, len),
            Err(e) => eprintln!("[{src}] parse error ({len} bytes): {e}"),
        }
    }
}

fn print_packet(packet: &Packet, src: std::net::SocketAddr, len: usize) {
    let hdr = packet.header();
    let ver = packet.version();
    let kind = packet.kind();
    println!(
        "[{src}] {ver} {kind:?} ({len} bytes) session={:#x} frame={} t={:.3}s",
        hdr.session_uid, hdr.frame_identifier, hdr.session_time,
    );

    match packet {
        Packet::F1_23(p) => print_v2023(p),
        Packet::F1_24(p) => print_v2024(p),
        Packet::F1_25(p) => print_v2025(p),
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// F1 23
// ---------------------------------------------------------------------------

fn print_v2023(p: &f1_telem::v2023::Packet) {
    use f1_telem::v2023::*;
    match p {
        Packet::Motion(d) => {
            for (i, c) in d.car_motion_data.iter().enumerate() {
                if c.world_position_x == 0.0 && c.world_position_y == 0.0 {
                    continue;
                }
                println!(
                    "  car[{i}] pos=({:.1}, {:.1}, {:.1}) vel=({:.1}, {:.1}, {:.1}) g=({:.2}, {:.2}, {:.2})",
                    c.world_position_x, c.world_position_y, c.world_position_z,
                    c.world_velocity_x, c.world_velocity_y, c.world_velocity_z,
                    c.g_force_lateral, c.g_force_longitudinal, c.g_force_vertical,
                );
            }
        }
        Packet::Session(d) => {
            println!(
                "  track={} type={} laps={} weather={} temp={}°C air={}°C formula={} safety_car={}",
                d.track_id,
                d.session_type,
                d.total_laps,
                d.weather,
                d.track_temperature,
                d.air_temperature,
                d.formula,
                d.safety_car_status,
            );
        }
        Packet::LapData(d) => {
            for (i, l) in d.lap_data.iter().enumerate() {
                if l.result_status == 0 {
                    continue;
                }
                println!(
                    "  car[{i}] P{} lap={} last={:.3}s cur={:.3}s dist={:.0}m pit={} status={}",
                    l.car_position,
                    l.current_lap_num,
                    l.last_lap_time_in_ms as f64 / 1000.0,
                    l.current_lap_time_in_ms as f64 / 1000.0,
                    l.lap_distance,
                    l.pit_status,
                    l.result_status,
                );
            }
        }
        Packet::Event(d) => {
            print_event_code(&d.event_code);
            print_event_23(&d.event_details);
        }
        Packet::Participants(d) => {
            println!("  active_cars={}", d.num_active_cars);
            for (i, p) in d.participants.iter().enumerate() {
                if p.name.is_empty() {
                    continue;
                }
                println!(
                    "  [{i}] \"{}\" team={} #{} ai={} platform={}",
                    p.name, p.team_id, p.race_number, p.ai_controlled, p.platform,
                );
            }
        }
        Packet::CarSetups(d) => {
            for (i, s) in d.car_setups.iter().enumerate() {
                if s.fuel_load == 0.0 && s.front_wing == 0 {
                    continue;
                }
                println!(
                    "  car[{i}] wing={}/{} brake_bias={}% fuel={:.1}kg tyre_psi=[{:.1},{:.1},{:.1},{:.1}]",
                    s.front_wing, s.rear_wing, s.brake_bias, s.fuel_load,
                    s.rear_left_tyre_pressure, s.rear_right_tyre_pressure,
                    s.front_left_tyre_pressure, s.front_right_tyre_pressure,
                );
            }
        }
        Packet::CarTelemetry(d) => {
            for (i, t) in d.car_telemetry_data.iter().enumerate() {
                if t.speed == 0 && t.engine_rpm == 0 {
                    continue;
                }
                println!(
                    "  car[{i}] {:.0}kph throttle={:.0}% brake={:.0}% gear={} rpm={} drs={}",
                    t.speed,
                    t.throttle * 100.0,
                    t.brake * 100.0,
                    t.gear,
                    t.engine_rpm,
                    t.drs,
                );
            }
        }
        Packet::CarStatus(d) => {
            for (i, s) in d.car_status_data.iter().enumerate() {
                if s.max_rpm == 0 {
                    continue;
                }
                println!(
                    "  car[{i}] fuel={:.1}kg/{:.1} laps tyre={}/{} age={} ers={:.0}J deploy={}",
                    s.fuel_in_tank,
                    s.fuel_remaining_laps,
                    s.actual_tyre_compound,
                    s.visual_tyre_compound,
                    s.tyres_age_laps,
                    s.ers_store_energy,
                    s.ers_deploy_mode,
                );
            }
        }
        Packet::FinalClassification(d) => {
            for (i, c) in d.classification_data.iter().enumerate() {
                if c.result_status == 0 {
                    continue;
                }
                println!(
                    "  [{i}] P{} laps={} grid={} pts={} pits={} best={:.3}s total={:.3}s penalties={}s status={}",
                    c.position, c.num_laps, c.grid_position, c.points,
                    c.num_pit_stops, c.best_lap_time_in_ms as f64 / 1000.0,
                    c.total_race_time, c.penalties_time, c.result_status,
                );
            }
        }
        Packet::LobbyInfo(d) => {
            println!("  players={}", d.num_players);
            for (i, l) in d.lobby_players.iter().enumerate() {
                if l.name.is_empty() {
                    continue;
                }
                println!(
                    "  [{i}] \"{}\" team={} #{} ready={} platform={}",
                    l.name, l.team_id, l.car_number, l.ready_status, l.platform,
                );
            }
        }
        Packet::CarDamage(d) => {
            for (i, c) in d.car_damage_data.iter().enumerate() {
                let any_wear = c.tyres_wear.iter().any(|&w| w > 0.0);
                if !any_wear && c.front_left_wing_damage == 0 {
                    continue;
                }
                println!(
                    "  car[{i}] tyres=[{:.0}%,{:.0}%,{:.0}%,{:.0}%] wing={}/{}% rear={}% floor={}% engine={}%",
                    c.tyres_wear[0], c.tyres_wear[1], c.tyres_wear[2], c.tyres_wear[3],
                    c.front_left_wing_damage, c.front_right_wing_damage,
                    c.rear_wing_damage, c.floor_damage, c.engine_damage,
                );
            }
        }
        Packet::SessionHistory(d) => {
            println!(
                "  car_idx={} laps={} stints={}",
                d.car_idx, d.num_laps, d.num_tyre_stints,
            );
            for (i, l) in d
                .lap_history_data
                .iter()
                .enumerate()
                .take(d.num_laps as usize)
            {
                println!(
                    "    lap[{i}] {:.3}s s1={:.3}s s2={:.3}s s3={:.3}s valid=0x{:02x}",
                    l.lap_time_in_ms as f64 / 1000.0,
                    l.sector1_time_in_ms as f64 / 1000.0,
                    l.sector2_time_in_ms as f64 / 1000.0,
                    l.sector3_time_in_ms as f64 / 1000.0,
                    l.lap_valid_bit_flags,
                );
            }
        }
        Packet::TyreSets(d) => {
            println!("  car_idx={} fitted_idx={}", d.car_idx, d.fitted_idx);
            for (i, t) in d.tyre_set_data.iter().enumerate() {
                if t.available == 0 && t.wear == 0 {
                    continue;
                }
                println!(
                    "    set[{i}] compound={}/{} wear={}% avail={} life={} fitted={}",
                    t.actual_tyre_compound,
                    t.visual_tyre_compound,
                    t.wear,
                    t.available,
                    t.life_span,
                    t.fitted,
                );
            }
        }
        Packet::MotionEx(d) => {
            println!(
                "  cog_height={:.3} vel=({:.2},{:.2},{:.2}) ang_vel=({:.3},{:.3},{:.3}) wheel_angle={:.3}",
                d.height_of_cog_above_ground,
                d.local_velocity_x, d.local_velocity_y, d.local_velocity_z,
                d.angular_velocity_x, d.angular_velocity_y, d.angular_velocity_z,
                d.front_wheels_angle,
            );
        }
        _ => {}
    }
}

fn print_event_code(code: &[u8; 4]) {
    let s = std::str::from_utf8(code).unwrap_or("????");
    print!("  event={s}");
}

fn print_unknown_event(code: &[u8; 4], payload: &[u8]) {
    let s = std::str::from_utf8(code).unwrap_or("????");
    println!(" Unknown code={s:?} payload={payload:02x?}");
}

fn print_event_23(ev: &f1_telem::v2023::EventDetails) {
    use f1_telem::v2023::EventDetails::*;
    match ev {
        SessionStarted => println!(" SessionStarted"),
        SessionEnded => println!(" SessionEnded"),
        FastestLap {
            vehicle_idx,
            lap_time,
        } => println!(" FastestLap car={vehicle_idx} time={lap_time:.3}s"),
        Retirement { vehicle_idx } => println!(" Retirement car={vehicle_idx}"),
        DrsEnabled => println!(" DrsEnabled"),
        DrsDisabled => println!(" DrsDisabled"),
        TeamMateInPits { vehicle_idx } => println!(" TeamMateInPits car={vehicle_idx}"),
        ChequeredFlag => println!(" ChequeredFlag"),
        RaceWinner { vehicle_idx } => println!(" RaceWinner car={vehicle_idx}"),
        Penalty {
            penalty_type,
            infringement_type,
            vehicle_idx,
            other_vehicle_idx,
            time,
            lap_num,
            places_gained,
        } => {
            println!(" Penalty car={vehicle_idx} other={other_vehicle_idx} type={penalty_type} infr={infringement_type} time={time}s lap={lap_num} places={places_gained}");
        }
        SpeedTrap {
            vehicle_idx, speed, ..
        } => println!(" SpeedTrap car={vehicle_idx} speed={speed:.1}kph"),
        StartLights { num_lights } => println!(" StartLights count={num_lights}"),
        LightsOut => println!(" LightsOut"),
        DriveThroughServed { vehicle_idx } => println!(" DriveThroughServed car={vehicle_idx}"),
        StopGoServed { vehicle_idx } => println!(" StopGoServed car={vehicle_idx}"),
        Flashback {
            flashback_frame_identifier,
            flashback_session_time,
        } => {
            println!(
                " Flashback frame={flashback_frame_identifier} t={flashback_session_time:.3}s"
            );
        }
        ButtonStatus { button_status } => println!(" ButtonStatus bits=0x{button_status:08x}"),
        RedFlag => println!(" RedFlag"),
        Overtake {
            overtaking_vehicle_idx,
            being_overtaken_vehicle_idx,
        } => {
            println!(" Overtake {overtaking_vehicle_idx} passes {being_overtaken_vehicle_idx}");
        }
        Unknown { code, payload } => print_unknown_event(code, payload),
        _ => println!(),
    }
}

// ---------------------------------------------------------------------------
// F1 24
// ---------------------------------------------------------------------------

fn print_v2024(p: &f1_telem::v2024::Packet) {
    use f1_telem::v2024::*;
    match p {
        Packet::Motion(d) => {
            for (i, c) in d.car_motion_data.iter().enumerate() {
                if c.world_position_x == 0.0 && c.world_position_y == 0.0 {
                    continue;
                }
                println!(
                    "  car[{i}] pos=({:.1},{:.1},{:.1}) g=({:.2},{:.2},{:.2})",
                    c.world_position_x,
                    c.world_position_y,
                    c.world_position_z,
                    c.g_force_lateral,
                    c.g_force_longitudinal,
                    c.g_force_vertical,
                );
            }
        }
        Packet::Session(d) => {
            println!(
                "  track={} type={} laps={} weather={} temp={}°C formula={} weekend_sessions={}",
                d.track_id,
                d.session_type,
                d.total_laps,
                d.weather,
                d.track_temperature,
                d.formula,
                d.num_sessions_in_weekend,
            );
        }
        Packet::LapData(d) => {
            for (i, l) in d.lap_data.iter().enumerate() {
                if l.result_status == 0 {
                    continue;
                }
                println!(
                    "  car[{i}] P{} lap={} last={:.3}s dist={:.0}m speed_trap={:.1}kph",
                    l.car_position,
                    l.current_lap_num,
                    l.last_lap_time_in_ms as f64 / 1000.0,
                    l.lap_distance,
                    l.speed_trap_fastest_speed,
                );
            }
        }
        Packet::Event(d) => {
            print_event_code(&d.event_code);
            print_event_24(&d.event_details);
        }
        Packet::Participants(d) => {
            println!("  active_cars={}", d.num_active_cars);
            for (i, p) in d.participants.iter().enumerate() {
                if p.name.is_empty() {
                    continue;
                }
                println!(
                    "  [{i}] \"{}\" team={} #{} tech_level={}",
                    p.name, p.team_id, p.race_number, p.tech_level,
                );
            }
        }
        Packet::CarSetups(d) => {
            println!("  next_front_wing={:.1}", d.next_front_wing_value);
            for (i, s) in d.car_setups.iter().enumerate() {
                if s.fuel_load == 0.0 && s.front_wing == 0 {
                    continue;
                }
                println!(
                    "  car[{i}] wing={}/{} engine_braking={}% fuel={:.1}kg",
                    s.front_wing, s.rear_wing, s.engine_braking, s.fuel_load,
                );
            }
        }
        Packet::CarTelemetry(d) => {
            for (i, t) in d.car_telemetry_data.iter().enumerate() {
                if t.speed == 0 {
                    continue;
                }
                println!(
                    "  car[{i}] {:.0}kph throttle={:.0}% brake={:.0}% gear={} rpm={}",
                    t.speed,
                    t.throttle * 100.0,
                    t.brake * 100.0,
                    t.gear,
                    t.engine_rpm,
                );
            }
        }
        Packet::CarStatus(d) => {
            for (i, s) in d.car_status_data.iter().enumerate() {
                if s.max_rpm == 0 {
                    continue;
                }
                println!(
                    "  car[{i}] fuel={:.1}kg tyre={}/{} age={}",
                    s.fuel_in_tank,
                    s.actual_tyre_compound,
                    s.visual_tyre_compound,
                    s.tyres_age_laps,
                );
            }
        }
        Packet::FinalClassification(d) => {
            for (i, c) in d.classification_data.iter().enumerate() {
                if c.result_status == 0 {
                    continue;
                }
                println!(
                    "  [{i}] P{} laps={} pts={} best={:.3}s status={}",
                    c.position,
                    c.num_laps,
                    c.points,
                    c.best_lap_time_in_ms as f64 / 1000.0,
                    c.result_status,
                );
            }
        }
        Packet::LobbyInfo(d) => {
            for (i, l) in d.lobby_players.iter().enumerate() {
                if l.name.is_empty() {
                    continue;
                }
                println!(
                    "  [{i}] \"{}\" team={} tech={}",
                    l.name, l.team_id, l.tech_level
                );
            }
        }
        Packet::CarDamage(d) => {
            for (i, c) in d.car_damage_data.iter().enumerate() {
                if c.tyres_wear.iter().all(|&w| w == 0.0) {
                    continue;
                }
                println!(
                    "  car[{i}] tyres=[{:.0}%,{:.0}%,{:.0}%,{:.0}%]",
                    c.tyres_wear[0], c.tyres_wear[1], c.tyres_wear[2], c.tyres_wear[3],
                );
            }
        }
        Packet::SessionHistory(d) => {
            println!("  car_idx={} laps={}", d.car_idx, d.num_laps);
        }
        Packet::TyreSets(d) => {
            println!("  car_idx={} fitted={}", d.car_idx, d.fitted_idx);
        }
        Packet::MotionEx(d) => {
            println!(
                "  cog={:.3} aero_h={:.3}/{:.3} roll={:.3}/{:.3} yaw={:.3}",
                d.height_of_cog_above_ground,
                d.front_aero_height,
                d.rear_aero_height,
                d.front_roll_angle,
                d.rear_roll_angle,
                d.chassis_yaw,
            );
        }
        Packet::TimeTrial(d) => {
            let pb = &d.personal_best_data_set;
            let rival = &d.rival_data_set;
            println!(
                "  PB: {:.3}s (car={} team={} valid={})",
                pb.lap_time_in_ms as f64 / 1000.0,
                pb.car_idx,
                pb.team_id,
                pb.valid,
            );
            println!(
                "  Rival: {:.3}s (car={} team={} valid={})",
                rival.lap_time_in_ms as f64 / 1000.0,
                rival.car_idx,
                rival.team_id,
                rival.valid,
            );
        }
        _ => {}
    }
}

fn print_event_24(ev: &f1_telem::v2024::EventDetails) {
    use f1_telem::v2024::EventDetails::*;
    match ev {
        SessionStarted => println!(" SessionStarted"),
        SessionEnded => println!(" SessionEnded"),
        FastestLap {
            vehicle_idx,
            lap_time,
        } => println!(" FastestLap car={vehicle_idx} {lap_time:.3}s"),
        Retirement { vehicle_idx } => println!(" Retirement car={vehicle_idx}"),
        DrsEnabled => println!(" DrsEnabled"),
        DrsDisabled => println!(" DrsDisabled"),
        TeamMateInPits { vehicle_idx } => println!(" TeamMateInPits car={vehicle_idx}"),
        ChequeredFlag => println!(" ChequeredFlag"),
        RaceWinner { vehicle_idx } => println!(" RaceWinner car={vehicle_idx}"),
        Penalty {
            vehicle_idx,
            penalty_type,
            infringement_type,
            ..
        } => {
            println!(" Penalty car={vehicle_idx} type={penalty_type} infr={infringement_type}");
        }
        SpeedTrap {
            vehicle_idx, speed, ..
        } => println!(" SpeedTrap car={vehicle_idx} {speed:.1}kph"),
        StartLights { num_lights } => println!(" StartLights {num_lights}"),
        LightsOut => println!(" LightsOut"),
        DriveThroughServed { vehicle_idx } => println!(" DriveThroughServed car={vehicle_idx}"),
        StopGoServed { vehicle_idx } => println!(" StopGoServed car={vehicle_idx}"),
        Flashback {
            flashback_frame_identifier,
            ..
        } => println!(" Flashback frame={flashback_frame_identifier}"),
        ButtonStatus { button_status } => println!(" Buttons 0x{button_status:08x}"),
        RedFlag => println!(" RedFlag"),
        Overtake {
            overtaking_vehicle_idx,
            being_overtaken_vehicle_idx,
        } => {
            println!(" Overtake {overtaking_vehicle_idx} passes {being_overtaken_vehicle_idx}");
        }
        SafetyCar {
            safety_car_type,
            event_type,
        } => {
            println!(" SafetyCar type={safety_car_type} event={event_type}");
        }
        Collision {
            vehicle1_idx,
            vehicle2_idx,
        } => {
            println!(" Collision {vehicle1_idx} <-> {vehicle2_idx}");
        }
        Unknown { code, payload } => print_unknown_event(code, payload),
        _ => println!(),
    }
}

// ---------------------------------------------------------------------------
// F1 25
// ---------------------------------------------------------------------------

fn print_v2025(p: &f1_telem::v2025::Packet) {
    use f1_telem::v2025::*;
    match p {
        Packet::Motion(d) => {
            for (i, c) in d.car_motion_data.iter().enumerate() {
                if c.world_position_x == 0.0 && c.world_position_y == 0.0 {
                    continue;
                }
                println!(
                    "  car[{i}] pos=({:.1},{:.1},{:.1}) g=({:.2},{:.2},{:.2})",
                    c.world_position_x,
                    c.world_position_y,
                    c.world_position_z,
                    c.g_force_lateral,
                    c.g_force_longitudinal,
                    c.g_force_vertical,
                );
            }
        }
        Packet::Session(d) => {
            println!(
                "  track={} type={} laps={} weather={} temp={}°C formula={}",
                d.track_id, d.session_type, d.total_laps, d.weather, d.track_temperature, d.formula,
            );
        }
        Packet::LapData(d) => {
            for (i, l) in d.lap_data.iter().enumerate() {
                if l.result_status == 0 {
                    continue;
                }
                println!(
                    "  car[{i}] P{} lap={} last={:.3}s dist={:.0}m",
                    l.car_position,
                    l.current_lap_num,
                    l.last_lap_time_in_ms as f64 / 1000.0,
                    l.lap_distance,
                );
            }
        }
        Packet::Event(d) => {
            print_event_code(&d.event_code);
            print_event_25(&d.event_details);
        }
        Packet::Participants(d) => {
            println!("  active_cars={}", d.num_active_cars);
            for (i, p) in d.participants.iter().enumerate() {
                if p.name.is_empty() {
                    continue;
                }
                println!(
                    "  [{i}] \"{}\" team={} #{} tech={} colours={}",
                    p.name, p.team_id, p.race_number, p.tech_level, p.num_colours,
                );
                for (ci, c) in p
                    .livery_colours
                    .iter()
                    .enumerate()
                    .take(p.num_colours as usize)
                {
                    println!("    colour[{ci}] rgb=({},{},{})", c.red, c.green, c.blue);
                }
            }
        }
        Packet::CarSetups(d) => {
            println!("  next_front_wing={:.1}", d.next_front_wing_value);
            for (i, s) in d.car_setups.iter().enumerate() {
                if s.fuel_load == 0.0 && s.front_wing == 0 {
                    continue;
                }
                println!(
                    "  car[{i}] wing={}/{} engine_braking={}% fuel={:.1}kg",
                    s.front_wing, s.rear_wing, s.engine_braking, s.fuel_load,
                );
            }
        }
        Packet::CarTelemetry(d) => {
            for (i, t) in d.car_telemetry_data.iter().enumerate() {
                if t.speed == 0 {
                    continue;
                }
                println!(
                    "  car[{i}] {:.0}kph throttle={:.0}% brake={:.0}% gear={} rpm={}",
                    t.speed,
                    t.throttle * 100.0,
                    t.brake * 100.0,
                    t.gear,
                    t.engine_rpm,
                );
            }
        }
        Packet::CarStatus(d) => {
            for (i, s) in d.car_status_data.iter().enumerate() {
                if s.max_rpm == 0 {
                    continue;
                }
                println!(
                    "  car[{i}] fuel={:.1}kg tyre={}/{} age={}",
                    s.fuel_in_tank,
                    s.actual_tyre_compound,
                    s.visual_tyre_compound,
                    s.tyres_age_laps,
                );
            }
        }
        Packet::FinalClassification(d) => {
            for (i, c) in d.classification_data.iter().enumerate() {
                if c.result_status == 0 {
                    continue;
                }
                println!(
                    "  [{i}] P{} laps={} pts={} best={:.3}s status={} reason={}",
                    c.position,
                    c.num_laps,
                    c.points,
                    c.best_lap_time_in_ms as f64 / 1000.0,
                    c.result_status,
                    c.result_reason,
                );
            }
        }
        Packet::LobbyInfo(d) => {
            for (i, l) in d.lobby_players.iter().enumerate() {
                if l.name.is_empty() {
                    continue;
                }
                println!(
                    "  [{i}] \"{}\" team={} tech={}",
                    l.name, l.team_id, l.tech_level
                );
            }
        }
        Packet::CarDamage(d) => {
            for (i, c) in d.car_damage_data.iter().enumerate() {
                if c.tyres_wear.iter().all(|&w| w == 0.0) {
                    continue;
                }
                println!(
                    "  car[{i}] tyres=[{:.0}%,{:.0}%,{:.0}%,{:.0}%] blisters=[{},{},{},{}]",
                    c.tyres_wear[0],
                    c.tyres_wear[1],
                    c.tyres_wear[2],
                    c.tyres_wear[3],
                    c.tyre_blisters[0],
                    c.tyre_blisters[1],
                    c.tyre_blisters[2],
                    c.tyre_blisters[3],
                );
            }
        }
        Packet::SessionHistory(d) => {
            println!("  car_idx={} laps={}", d.car_idx, d.num_laps);
        }
        Packet::TyreSets(d) => {
            println!("  car_idx={} fitted={}", d.car_idx, d.fitted_idx);
        }
        Packet::MotionEx(d) => {
            println!(
                "  cog={:.3} aero_h={:.3}/{:.3} roll={:.3}/{:.3} yaw={:.3} pitch={:.3}",
                d.height_of_cog_above_ground,
                d.front_aero_height,
                d.rear_aero_height,
                d.front_roll_angle,
                d.rear_roll_angle,
                d.chassis_yaw,
                d.chassis_pitch,
            );
            println!(
                "  camber=[{:.3},{:.3},{:.3},{:.3}] gain=[{:.3},{:.3},{:.3},{:.3}]",
                d.wheel_camber[0],
                d.wheel_camber[1],
                d.wheel_camber[2],
                d.wheel_camber[3],
                d.wheel_camber_gain[0],
                d.wheel_camber_gain[1],
                d.wheel_camber_gain[2],
                d.wheel_camber_gain[3],
            );
        }
        Packet::TimeTrial(d) => {
            let pb = &d.personal_best_data_set;
            let rival = &d.rival_data_set;
            println!(
                "  PB: {:.3}s  Rival: {:.3}s",
                pb.lap_time_in_ms as f64 / 1000.0,
                rival.lap_time_in_ms as f64 / 1000.0,
            );
        }
        Packet::LapPositions(d) => {
            println!("  laps={} start={}", d.num_laps, d.lap_start);
            for lap in 0..d.num_laps.min(50) as usize {
                let offset = lap * 22;
                let positions: Vec<u8> = (0..22)
                    .map(|car| d.position_for_vehicle_idx[offset + car])
                    .filter(|&p| p > 0)
                    .collect();
                if !positions.is_empty() {
                    println!(
                        "    lap[{}] positions={:?}",
                        lap + d.lap_start as usize,
                        positions
                    );
                }
            }
        }
        _ => {}
    }
}

fn print_event_25(ev: &f1_telem::v2025::EventDetails) {
    use f1_telem::v2025::EventDetails::*;
    match ev {
        SessionStarted => println!(" SessionStarted"),
        SessionEnded => println!(" SessionEnded"),
        FastestLap {
            vehicle_idx,
            lap_time,
        } => println!(" FastestLap car={vehicle_idx} {lap_time:.3}s"),
        Retirement {
            vehicle_idx,
            reason,
        } => println!(" Retirement car={vehicle_idx} reason={reason}"),
        DrsEnabled => println!(" DrsEnabled"),
        DrsDisabled { reason } => println!(" DrsDisabled reason={reason}"),
        TeamMateInPits { vehicle_idx } => println!(" TeamMateInPits car={vehicle_idx}"),
        ChequeredFlag => println!(" ChequeredFlag"),
        RaceWinner { vehicle_idx } => println!(" RaceWinner car={vehicle_idx}"),
        Penalty {
            vehicle_idx,
            penalty_type,
            infringement_type,
            ..
        } => {
            println!(" Penalty car={vehicle_idx} type={penalty_type} infr={infringement_type}");
        }
        SpeedTrap {
            vehicle_idx, speed, ..
        } => println!(" SpeedTrap car={vehicle_idx} {speed:.1}kph"),
        StartLights { num_lights } => println!(" StartLights {num_lights}"),
        LightsOut => println!(" LightsOut"),
        DriveThroughServed { vehicle_idx } => println!(" DriveThroughServed car={vehicle_idx}"),
        StopGoServed {
            vehicle_idx,
            stop_time,
        } => {
            println!(" StopGoServed car={vehicle_idx} stop_time={stop_time:.1}s");
        }
        Flashback {
            flashback_frame_identifier,
            ..
        } => println!(" Flashback frame={flashback_frame_identifier}"),
        ButtonStatus { button_status } => println!(" Buttons 0x{button_status:08x}"),
        RedFlag => println!(" RedFlag"),
        Overtake {
            overtaking_vehicle_idx,
            being_overtaken_vehicle_idx,
        } => {
            println!(" Overtake {overtaking_vehicle_idx} passes {being_overtaken_vehicle_idx}");
        }
        SafetyCar {
            safety_car_type,
            event_type,
        } => {
            println!(" SafetyCar type={safety_car_type} event={event_type}");
        }
        Collision {
            vehicle1_idx,
            vehicle2_idx,
        } => {
            println!(" Collision {vehicle1_idx} <-> {vehicle2_idx}");
        }
        Unknown { code, payload } => print_unknown_event(code, payload),
        _ => println!(),
    }
}
