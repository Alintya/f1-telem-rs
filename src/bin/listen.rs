use std::fmt::Display;
use std::net::UdpSocket;

use f1_telem::common::*;
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
    packet.visit(&mut Printer);
}

/// ` name=value` for fields only some game versions send; empty otherwise.
fn opt(name: &str, value: Option<impl Display>) -> String {
    value.map_or_else(String::new, |v| format!(" {name}={v}"))
}

fn secs(ms: u32) -> f64 {
    f64::from(ms) / 1000.0
}

/// Sector time from either the single-field (F1 23) or minutes + ms (F1 24+) layout.
fn sector_secs(ms: Option<u16>, ms_part: Option<u16>, minutes_part: Option<u8>) -> f64 {
    let ms = ms.or(ms_part).map_or(0, u32::from);
    secs(ms + u32::from(minutes_part.unwrap_or(0)) * 60_000)
}

/// Prints every packet kind through the version-independent `common` traits,
/// so one implementation covers all supported game versions.
struct Printer;

impl PacketVisitor for Printer {
    fn motion<P: PacketMotion>(&mut self, p: &P) {
        for (i, c) in p.car_motion_data().iter().enumerate() {
            if c.world_position_x() == 0.0 && c.world_position_y() == 0.0 {
                continue;
            }
            println!(
                "  car[{i}] pos=({:.1},{:.1},{:.1}) vel=({:.1},{:.1},{:.1}) g=({:.2},{:.2},{:.2})",
                c.world_position_x(),
                c.world_position_y(),
                c.world_position_z(),
                c.world_velocity_x(),
                c.world_velocity_y(),
                c.world_velocity_z(),
                c.g_force_lateral(),
                c.g_force_longitudinal(),
                c.g_force_vertical(),
            );
        }
    }

    fn session<P: PacketSession>(&mut self, p: &P) {
        println!(
            "  track={} type={} laps={} weather={} temp={}°C air={}°C formula={} safety_car={}{}",
            p.track_id(),
            p.session_type(),
            p.total_laps(),
            p.weather(),
            p.track_temperature(),
            p.air_temperature(),
            p.formula(),
            p.safety_car_status(),
            opt(
                "start_reaction",
                p.start_reaction_time().map(|t| format!("{t:.3}s"))
            ),
        );
        if let (Some(full), Some(zones)) =
            (p.num_active_aero_zones_full(), p.active_aero_zones_full())
        {
            for z in zones.iter().take(usize::from(full)) {
                println!("    active_aero {:.3}..{:.3}", z.zone_start(), z.zone_end());
            }
        }
        if let (Some(n), Some(zones)) = (p.num_drs_zones(), p.drs_zones()) {
            for z in zones.iter().take(usize::from(n)) {
                println!("    drs {:.3}..{:.3}", z.zone_start(), z.zone_end());
            }
        }
    }

    fn lap_data<P: PacketLapData>(&mut self, p: &P) {
        for (i, l) in p.lap_data().iter().enumerate() {
            if l.result_status() == 0 {
                continue;
            }
            println!(
                "  car[{i}] P{} lap={} last={:.3}s cur={:.3}s dist={:.0}m pit={} status={}",
                l.car_position(),
                l.current_lap_num(),
                secs(l.last_lap_time_in_ms()),
                secs(l.current_lap_time_in_ms()),
                l.lap_distance(),
                l.pit_status(),
                l.result_status(),
            );
        }
    }

    fn event<P: PacketEvent>(&mut self, p: &P) {
        let code = p.event_code();
        print!("  event={}", std::str::from_utf8(&code).unwrap_or("????"));
        match p.event_details() {
            EventDetails::Unknown { code, payload } => println!(
                " Unknown code={:?} payload={payload:02x?}",
                std::str::from_utf8(&code).unwrap_or("????")
            ),
            EventDetails::FastestLap { vehicle_idx, lap_time } => {
                println!(" FastestLap car={vehicle_idx} {lap_time:.3}s");
            }
            EventDetails::Retirement { vehicle_idx, reason } => {
                println!(" Retirement car={vehicle_idx}{}", opt("reason", reason));
            }
            EventDetails::DrsDisabled { reason } => println!(" DrsDisabled{}", opt("reason", reason)),
            EventDetails::Penalty {
                vehicle_idx,
                penalty_type,
                infringement_type,
                time,
                ..
            } => println!(" Penalty car={vehicle_idx} type={penalty_type} infr={infringement_type} time={time}s"),
            EventDetails::SpeedTrap { vehicle_idx, speed, .. } => {
                println!(" SpeedTrap car={vehicle_idx} {speed:.1}kph");
            }
            EventDetails::StopGoServed { vehicle_idx, stop_time } => {
                println!(" StopGoServed car={vehicle_idx}{}", opt("stop_time", stop_time));
            }
            EventDetails::Flashback { flashback_frame_identifier, .. } => {
                println!(" Flashback frame={flashback_frame_identifier}");
            }
            EventDetails::ButtonStatus { button_status } => println!(" Buttons 0x{button_status:08x}"),
            EventDetails::Overtake {
                overtaking_vehicle_idx,
                being_overtaken_vehicle_idx,
            } => println!(" Overtake {overtaking_vehicle_idx} passes {being_overtaken_vehicle_idx}"),
            EventDetails::Collision {
                vehicle1_idx,
                vehicle2_idx,
                severity,
            } => println!(" Collision {vehicle1_idx} <-> {vehicle2_idx}{}", opt("severity", severity)),
            // Remaining events print their name and fields as-is.
            other => println!(" {other:?}"),
        }
    }

    fn participants<P: PacketParticipants>(&mut self, p: &P) {
        println!("  active_cars={}", p.num_active_cars());
        for (i, d) in p.participants().iter().enumerate() {
            if d.name().is_empty() {
                continue;
            }
            println!(
                "  [{i}] \"{}\" driver={} team={} #{} ai={} platform={}{}",
                d.name(),
                d.driver_id(),
                d.team_id(),
                d.race_number(),
                d.ai_controlled(),
                d.platform(),
                opt("tech", d.tech_level()),
            );
            if let (Some(n), Some(colours)) = (d.num_colours(), d.livery_colours()) {
                for (ci, c) in colours.iter().enumerate().take(usize::from(n)) {
                    println!(
                        "    colour[{ci}] rgb=({},{},{})",
                        c.red(),
                        c.green(),
                        c.blue()
                    );
                }
            }
        }
    }

    fn car_setups<P: PacketCarSetups>(&mut self, p: &P) {
        if let Some(wing) = p.next_front_wing_value() {
            println!("  next_front_wing={wing:.1}");
        }
        for (i, s) in p.car_setups().iter().enumerate() {
            if s.fuel_load() == 0.0 && s.front_wing() == 0 {
                continue;
            }
            println!(
                "  car[{i}] wing={}/{} brake_bias={}% fuel={:.1}kg{}",
                s.front_wing(),
                s.rear_wing(),
                s.brake_bias(),
                s.fuel_load(),
                opt("engine_braking", s.engine_braking()),
            );
        }
    }

    fn car_telemetry<P: PacketCarTelemetry>(&mut self, p: &P) {
        for (i, t) in p.car_telemetry_data().iter().enumerate() {
            if t.speed() == 0 && t.engine_rpm() == 0 {
                continue;
            }
            println!(
                "  car[{i}] {}kph throttle={:.0}% brake={:.0}% gear={} rpm={} drs={} engine={}°C",
                t.speed(),
                t.throttle() * 100.0,
                t.brake() * 100.0,
                t.gear(),
                t.engine_rpm(),
                t.drs(),
                t.engine_temperature(),
            );
        }
    }

    fn car_status<P: PacketCarStatus>(&mut self, p: &P) {
        for (i, s) in p.car_status_data().iter().enumerate() {
            if s.max_rpm() == 0 {
                continue;
            }
            println!(
                "  car[{i}] fuel={:.1}kg/{:.1} laps tyre={}/{} age={} ers={:.0}J deploy={}{}",
                s.fuel_in_tank(),
                s.fuel_remaining_laps(),
                s.actual_tyre_compound(),
                s.visual_tyre_compound(),
                s.tyres_age_laps(),
                s.ers_store_energy(),
                s.ers_deploy_mode(),
                opt(
                    "harvest_limit",
                    s.ers_harvest_limit_per_lap().map(|j| format!("{j:.0}J"))
                ),
            );
        }
    }

    fn final_classification<P: PacketFinalClassification>(&mut self, p: &P) {
        for (i, c) in p.classification_data().iter().enumerate() {
            if c.result_status() == 0 {
                continue;
            }
            println!(
                "  [{i}] P{} laps={} grid={} pts={} pits={} best={:.3}s total={:.3}s status={}{}",
                c.position(),
                c.num_laps(),
                c.grid_position(),
                c.points(),
                c.num_pit_stops(),
                secs(c.best_lap_time_in_ms()),
                c.total_race_time(),
                c.result_status(),
                opt("reason", c.result_reason()),
            );
        }
    }

    fn lobby_info<P: PacketLobbyInfo>(&mut self, p: &P) {
        println!("  players={}", p.num_players());
        for (i, l) in p.lobby_players().iter().enumerate() {
            if l.name().is_empty() {
                continue;
            }
            println!(
                "  [{i}] \"{}\" team={} #{} ready={} platform={}{}",
                l.name(),
                l.team_id(),
                l.car_number(),
                l.ready_status(),
                l.platform(),
                opt("tech", l.tech_level()),
            );
        }
    }

    fn car_damage<P: PacketCarDamage>(&mut self, p: &P) {
        for (i, c) in p.car_damage_data().iter().enumerate() {
            let w = c.tyres_wear();
            if w.iter().all(|&x| x == 0.0) && c.front_left_wing_damage() == 0 {
                continue;
            }
            println!(
                "  car[{i}] tyres=[{:.0}%,{:.0}%,{:.0}%,{:.0}%] wing={}/{}% rear={}% floor={}% engine={}%{}",
                w[0],
                w[1],
                w[2],
                w[3],
                c.front_left_wing_damage(),
                c.front_right_wing_damage(),
                c.rear_wing_damage(),
                c.floor_damage(),
                c.engine_damage(),
                opt("blisters", c.tyre_blisters().map(|b| format!("{b:?}"))),
            );
        }
    }

    fn session_history<P: PacketSessionHistory>(&mut self, p: &P) {
        println!(
            "  car_idx={} laps={} stints={}",
            p.car_idx(),
            p.num_laps(),
            p.num_tyre_stints()
        );
        for (i, l) in p
            .lap_history_data()
            .iter()
            .enumerate()
            .take(usize::from(p.num_laps()))
        {
            println!(
                "    lap[{i}] {:.3}s s1={:.3}s s2={:.3}s s3={:.3}s valid=0x{:02x}",
                secs(l.lap_time_in_ms()),
                sector_secs(
                    l.sector1_time_in_ms(),
                    l.sector1_time_ms_part(),
                    l.sector1_time_minutes_part()
                ),
                sector_secs(
                    l.sector2_time_in_ms(),
                    l.sector2_time_ms_part(),
                    l.sector2_time_minutes_part()
                ),
                sector_secs(
                    l.sector3_time_in_ms(),
                    l.sector3_time_ms_part(),
                    l.sector3_time_minutes_part()
                ),
                l.lap_valid_bit_flags(),
            );
        }
    }

    fn tyre_sets<P: PacketTyreSets>(&mut self, p: &P) {
        println!("  car_idx={} fitted_idx={}", p.car_idx(), p.fitted_idx());
        for (i, t) in p.tyre_set_data().iter().enumerate() {
            if t.available() == 0 && t.wear() == 0 {
                continue;
            }
            println!(
                "    set[{i}] compound={}/{} wear={}% avail={} life={} fitted={}",
                t.actual_tyre_compound(),
                t.visual_tyre_compound(),
                t.wear(),
                t.available(),
                t.life_span(),
                t.fitted(),
            );
        }
    }

    fn motion_ex<P: PacketMotionEx>(&mut self, p: &P) {
        println!(
            "  cog_height={:.3} vel=({:.2},{:.2},{:.2}) ang_vel=({:.3},{:.3},{:.3}) wheel_angle={:.3}{}{}",
            p.height_of_cog_above_ground(),
            p.local_velocity_x(),
            p.local_velocity_y(),
            p.local_velocity_z(),
            p.angular_velocity_x(),
            p.angular_velocity_y(),
            p.angular_velocity_z(),
            p.front_wheels_angle(),
            opt("chassis_yaw", p.chassis_yaw().map(|v| format!("{v:.3}"))),
            opt("chassis_pitch", p.chassis_pitch().map(|v| format!("{v:.3}"))),
        );
    }

    fn time_trial<P: PacketTimeTrial>(&mut self, p: &P) {
        println!(
            "  session_best={:.3}s PB={:.3}s rival={:.3}s",
            secs(p.player_session_best_data_set().lap_time_in_ms()),
            secs(p.personal_best_data_set().lap_time_in_ms()),
            secs(p.rival_data_set().lap_time_in_ms()),
        );
    }

    fn lap_positions<P: PacketLapPositions>(&mut self, p: &P) {
        let positions = p.position_for_vehicle_idx();
        let cars = positions.len() / 50; // 50 laps of per-car positions
        println!("  laps={} start={}", p.num_laps(), p.lap_start());
        for (lap, row) in positions
            .chunks(cars)
            .take(usize::from(p.num_laps()))
            .enumerate()
        {
            let row: Vec<u8> = row.iter().copied().filter(|&pos| pos > 0).collect();
            if !row.is_empty() {
                println!(
                    "    lap[{}] positions={row:?}",
                    lap + usize::from(p.lap_start())
                );
            }
        }
    }

    fn car_telemetry2<P: PacketCarTelemetry2>(&mut self, p: &P) {
        for (i, t) in p.car_telemetry2_data().iter().enumerate() {
            if t.regulations_2026() == 0 && t.active_aero_available() == 0 {
                continue;
            }
            println!(
                "  car[{i}] aero_mode={} aero_avail={} in={}m overtake={}/{} in={}m wrong_way={}",
                t.active_aero_mode(),
                t.active_aero_available(),
                t.active_aero_activation_distance(),
                t.overtake_available(),
                t.overtake_active(),
                t.overtake_activation_distance(),
                t.driving_wrong_way(),
            );
        }
    }
}
