# f1-telem

High-performance, zero-dependency Rust parser for EA Sports F1 game UDP telemetry. Supports **F1 23**, **F1 24**, **F1 25**, and the **F1 25 2026 Season Pack** (packet format 2026, exposed as `GameVersion::F1_26` / module `v2026`).

Packet definitions are generated at compile time from YAML specifications — no hand-written decoding logic, no runtime dependencies, no allocations on the parse path.

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
f1-telem = { path = "." }  # or publish/git reference
```

Parse a raw UDP datagram and handle it the same way for every game version through the `common` view:

```rust
use f1_telem::common::*;
use f1_telem::parse_packet;

struct Dashboard;

impl PacketVisitor for Dashboard {
    fn car_telemetry<P: PacketCarTelemetry>(&mut self, packet: &P) {
        let player = packet.header().player_car_index as usize;
        let car = &packet.car_telemetry_data()[player];
        println!("{} km/h, gear {}", car.speed(), car.gear());
    }

    fn event<P: PacketEvent>(&mut self, packet: &P) {
        if let EventDetails::Collision { vehicle1_idx, vehicle2_idx, severity } = packet.event_details() {
            println!("collision {vehicle1_idx} <-> {vehicle2_idx} (severity {severity:?})");
        }
    }
    // Every other packet kind defaults to doing nothing.
}

let data: &[u8] = /* recv_from(...) */;
match parse_packet(data) {
    Ok(packet) => packet.visit(&mut Dashboard),
    Err(e) => eprintln!("parse error: {e}"),
}
```

Each struct and packet has a trait of the same name in `common`, implemented by every version. Accessors return a type that fits all versions: narrower integers are widened (`team_id()` is `u16`), quantised values are scaled (`g_force_lateral()` is `f32` in g even where the game sends `i16`), arrays whose length differs are slices (22 vs 24 cars), names are `Cow<str>`, and fields a version lacks return `Option` (`None` for that version). `common::EventDetails` is the union of all versions' events. New versions and packet kinds never break a `PacketVisitor`.

For exact wire values, match on the version-specific types instead:

```rust
use f1_telem::{parse_packet, Packet};

match parse_packet(data) {
    Ok(Packet::F1_26(packet)) => {
        use f1_telem::v2026::Packet as P;
        if let P::Motion(m) = packet {
            let raw: i16 = m.car_motion_data[0].g_force_lateral; // thousandths of a g
        }
    }
    Ok(_) => { /* other versions */ }
    Err(e) => eprintln!("parse error: {e}"),
}
```

### Included listener binary

A simple CLI binary is included that binds a UDP socket and prints decoded packets to stdout. It is written once against the `common` view, so it prints every supported version:

```sh
cargo run                        # listens on 0.0.0.0:20777
cargo run -- 192.168.1.50:20777  # custom bind address
```

## Supported packet types

| ID | Packet | F1 23 | F1 24 | F1 25 | 2026 |
|----|--------|:-----:|:-----:|:-----:|:----:|
| 0  | Motion | x | x | x | x |
| 1  | Session | x | x | x | x |
| 2  | Lap Data | x | x | x | x |
| 3  | Event | x | x | x | x |
| 4  | Participants | x | x | x | x |
| 5  | Car Setups | x | x | x | x |
| 6  | Car Telemetry | x | x | x | x |
| 7  | Car Status | x | x | x | x |
| 8  | Final Classification | x | x | x | x |
| 9  | Lobby Info | x | x | x | x |
| 10 | Car Damage | x | x | x | x |
| 11 | Session History | x | x | x | x |
| 12 | Tyre Sets | x | x | x | x |
| 13 | Motion Ex | x | x | x | x |
| 14 | Time Trial | | x | x | x |
| 15 | Lap Positions | | | x | x |
| 16 | Car Telemetry 2 | | | | x |

2026 Season Pack notes: car arrays hold 24 cars; Motion g-force fields are quantised `i16` (physical value = raw × 0.001); the partial-mode / overtake-mode events (`PMEN`, `PMDI`, `OVEN`, `OVDI`) come from the Season 8 revision of EA's document.

## How it works

```
spec/f1_20XX.yaml ──► build.rs ──► $OUT_DIR/generated.rs ──► src/generated.rs (include!)
```

1. YAML files in `spec/` declare every packet's layout: struct names, field names, types, array sizes, event codes, and expected total sizes. `f1_2023.yaml` is complete; each later year `extends` the previous one and lists only what changed.
2. `build.rs` loads every spec in the directory, resolves inheritance, validates that computed sizes match declared sizes (header + body) and that event payloads fit, and emits one module per version (`v2023` … `v2026`) plus the `GameVersion`, `PacketKind` and `Packet` enums.
3. At runtime, `parse_packet` reads the 29-byte header to determine game version and packet type, then dispatches to the generated decoder.

Name fields are `FixedStr<N>`: the raw NUL-padded bytes stored inline. Use `.as_str()` (UTF-8 checked) or `.to_string_lossy()`.

### Adding a game version

Create `spec/f1_YYYY.yaml` with `extends: f1_<previous>` and only the changed structs, packets (by `id`) and events (by `code`). The new `vYYYY` module, `GameVersion::F1_YY` and dispatch are generated; no Rust changes are needed in the library.

## Error handling

`parse_packet` returns `Result<Packet, ParseError>`. Failure modes:

| Variant | Meaning |
|---------|---------|
| `BufferTooShort` | Datagram shorter than required |
| `UnsupportedFormat` | `packet_format` not 2023–2026 |
| `UnknownPacketId` | Packet ID not valid for the game version |
| `SizeMismatch` | Datagram shorter than the packet's declared size |

Unrecognized event codes are not errors: they decode to `EventDetails::Unknown { code, payload }`. All public enums are `#[non_exhaustive]`, so new versions, packets and events are additive.

## Building & testing

```sh
cargo build   # triggers codegen from spec/*.yaml
cargo test    # unit tests (header, reader) + integration tests (all packet types)
```

## License

MIT
