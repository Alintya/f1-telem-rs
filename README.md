# f1-telem

High-performance, zero-dependency Rust parser for EA Sports F1 game UDP telemetry. Supports **F1 23**, **F1 24**, and **F1 25**.

Packet definitions are generated at compile time from YAML specifications — no hand-written decoding logic, no runtime dependencies, no allocations on the parse path.

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
f1-telem = { path = "." }  # or publish/git reference
```

Parse a raw UDP datagram:

```rust
use f1_telem::{parse_packet, Packet};

let data: &[u8] = /* recv_from(...) */;

match parse_packet(data) {
    Ok(Packet::F1_25(packet)) => {
        let header = packet.header();
        println!("session {}, frame {}", header.session_uid, header.frame_identifier);

        // Match specific packet types
        use f1_telem::v2025::Packet as P;
        match packet {
            P::CarTelemetry(telemetry) => { /* ... */ }
            P::LapData(laps) => { /* ... */ }
            _ => {}
        }
    }
    Ok(Packet::F1_24(packet)) => { /* ... */ }
    Ok(Packet::F1_23(packet)) => { /* ... */ }
    Err(e) => eprintln!("parse error: {e}"),
}
```

### Included listener binary

A simple CLI binary is included that binds a UDP socket and prints decoded packets to stdout:

```sh
cargo run                        # listens on 0.0.0.0:20777
cargo run -- 192.168.1.50:20777  # custom bind address
```

## Supported packet types

| ID | Packet | F1 23 | F1 24 | F1 25 |
|----|--------|:-----:|:-----:|:-----:|
| 0  | Motion | x | x | x |
| 1  | Session | x | x | x |
| 2  | Lap Data | x | x | x |
| 3  | Event | x | x | x |
| 4  | Participants | x | x | x |
| 5  | Car Setups | x | x | x |
| 6  | Car Telemetry | x | x | x |
| 7  | Car Status | x | x | x |
| 8  | Final Classification | x | x | x |
| 9  | Lobby Info | x | x | x |
| 10 | Car Damage | x | x | x |
| 11 | Session History | x | x | x |
| 12 | Tyre Sets | x | x | x |
| 13 | Motion Ex | x | x | x |
| 14 | Time Trial | | x | x |
| 15 | Lap Positions | | | x |

## How it works

```
spec/f1_20XX.yaml ──► build.rs ──► $OUT_DIR/generated.rs ──► src/generated.rs (include!)
```

1. YAML files in `spec/` declare every packet's layout: struct names, field names, types, array sizes, event codes, and expected total sizes.
2. `build.rs` deserializes the specs, resolves struct references, validates that computed sizes match declared sizes (header + body), and emits Rust modules (`v2023`, `v2024`, `v2025`) with struct definitions and `decode(&mut Reader)` methods.
3. At runtime, `parse_packet` reads the 29-byte header to determine game version and packet type, then dispatches to the generated decoder.

## Error handling

`parse_packet` returns `Result<Packet, ParseError>`. Failure modes:

| Variant | Meaning |
|---------|---------|
| `BufferTooShort` | Datagram shorter than required |
| `UnsupportedFormat` | `packet_format` not 2023/2024/2025 |
| `UnknownPacketId` | Packet ID not valid for the game version |
| `UnknownEventCode` | Unrecognized 4-byte event string code |
| `InvalidUtf8` | Name field contains invalid UTF-8 |
| `SizeMismatch` | Datagram length doesn't match expected packet size |

## Building & testing

```sh
cargo build   # triggers codegen from spec/*.yaml
cargo test    # unit tests (header, reader) + integration tests (all packet types)
```

## License

MIT
