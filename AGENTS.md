# Repository Guidelines

## Project Overview

`f1-telem` is a zero-dependency, zero-allocation Rust library that parses UDP telemetry datagrams from EA Sports F1 23, F1 24, and F1 25. Packet struct definitions are generated at compile time from YAML specifications via `build.rs`—there is no hand-written decoding logic for individual packet types.

## Architecture & Data Flow

```
spec/*.yaml ──► build.rs (serde_yaml) ──► $OUT_DIR/generated.rs
                                              │
src/generated.rs  ◄── include!() ────────────┘
       │
src/lib.rs  ──► parse_packet(&[u8]) -> Result<Packet, ParseError>
       │
       ├─ header.rs   – parse 29-byte PacketHeader, extract packet_format
       ├─ reader.rs   – Reader cursor: little-endian, sequential, no-alloc
       ├─ version.rs  – GameVersion / PacketType enums, ID ↔ enum conversion
       └─ error.rs    – ParseError enum (manual Display/Error impls)
```

**Runtime parse path:**

1. `parse_header(data)` validates ≥29 bytes, reads `PacketHeader` via `Reader`.
2. `parse_packet(data)` dispatches on `header.packet_format` (2023/2024/2025) to `v20XX::Packet::decode`.
3. Version-specific `decode` reads the header, matches `packet_id`, and calls the generated struct decoder that reads fields sequentially in little-endian order.
4. Returns `Packet::F1_23(v2023::Packet)` | `Packet::F1_24(…)` | `Packet::F1_25(…)`.

## Key Directories

| Path | Purpose |
|---|---|
| `src/` | Library crate root and core modules |
| `src/bin/listen.rs` | Example binary—binds UDP socket, parses & prints packets to stdout |
| `spec/` | YAML packet specifications per game year (source of truth for codegen) |
| `build.rs` | Build script: YAML → Rust codegen with compile-time size validation |
| `tests/` | Integration tests for packet parsing |
| `docs/` | Official EA/Codemasters specification documents (PDF, DOCX, TXT) |

## Development Commands

```sh
cargo build          # Triggers build.rs codegen from spec/*.yaml
cargo test           # Runs unit tests (header, reader) + integration tests (packet_parse)
cargo run -- [ADDR]  # Runs listen binary; defaults to 0.0.0.0:20777 (UDP)
```

No linter, formatter config, or CI pipeline is checked in. Use `cargo fmt` and `cargo clippy` conventionally.

## Code Conventions & Common Patterns

### Naming
- Structs: `PascalCase` matching the telemetry spec (e.g., `PacketCarTelemetryData`).
- Fields: `snake_case`, translated from the C++ `m_` prefix originals (e.g., `m_aiControlled` → `ai_controlled`).
- Modules: version modules named `v2023`, `v2024`, `v2025`.

### Error Handling
- Custom `ParseError` enum in `src/error.rs` with manual `std::fmt::Display` and `std::error::Error` impls (no `thiserror`/`anyhow`).
- Crate-level `type Result<T> = std::result::Result<T, ParseError>` alias.
- Variants: `BufferTooShort`, `UnsupportedFormat(u16)`, `UnknownPacketId(u8)`, `UnknownEventCode([u8; 4])`, `Utf8Error`.

### No External Runtime Dependencies
- Zero crate dependencies at runtime. `serde` and `serde_yaml` are build-dependencies only.
- No async runtime—the listen binary uses blocking `std::net::UdpSocket`.
- No unsafe code or FFI.

### Reader Pattern
`Reader` (`src/reader.rs`) wraps `&[u8]` with a position cursor. Methods: `read_u8`, `read_u16`, `read_u32`, `read_u64`, `read_i8`, `read_i16`, `read_f32`, `read_f64`, `read_string(len)`, `read_array`. All little-endian, all return `Result<T, ParseError>`. No allocations on the parse hot path.

### Code Generation
- `build.rs` deserializes YAML specs into intermediate structs, computes and validates packet sizes (header 29 bytes + body == declared size), then emits Rust modules with `struct` definitions and `decode(&mut Reader)` methods.
- Generated code lives in `$OUT_DIR/generated.rs`, pulled in via `include!` in `src/generated.rs`.
- `cargo:rerun-if-changed` is set for `build.rs` and each `spec/*.yaml` file.

### YAML Spec Schema
Each `spec/f1_YYYY.yaml` declares:
- `version`: game year identifier
- `header`: common 29-byte packet header fields
- `structs`: reusable sub-structures (field name, type, optional array size)
- `packets`: top-level packet definitions (ID, total size, body fields referencing structs)
- `events`: event code → payload mappings for the Event packet type

## Important Files

| File | Role |
|---|---|
| `src/lib.rs` | Crate root: module declarations, `Packet` enum, `parse_packet()` entry point |
| `src/generated.rs` | `include!` bridge to build-generated version modules |
| `src/reader.rs` | Zero-alloc binary cursor used by all decoders |
| `src/header.rs` | `PacketHeader` struct, `parse_header()`, `HEADER_SIZE` constant (29) |
| `src/error.rs` | `ParseError` enum and `Result<T>` alias |
| `src/version.rs` | `GameVersion`, `PacketType` enums with ID conversion |
| `src/bin/listen.rs` | CLI binary: UDP recv → `parse_packet` → formatted stdout |
| `build.rs` | Codegen: YAML specs → Rust decoder modules |
| `spec/f1_2025.yaml` | Latest game spec (F1 25); others follow same schema |

## Runtime/Tooling Preferences

- **Language:** Rust, edition 2021.
- **No MSRV** specified.
- **No runtime dependencies.** Build deps only: `serde`, `serde_yaml` (for `build.rs`).
- **No feature flags** defined.
- **No workspace**—single crate.
- **Package manager:** Cargo (standard).

## Testing & QA

### Test Structure
- **Unit tests:** Inline `#[cfg(test)] mod tests` in `src/header.rs` and `src/reader.rs`.
- **Integration tests:** `tests/packet_parse.rs`—imports `f1_telem::*` and exercises the public `parse_packet` API.

### Test Data
All packet bytes are synthetically constructed in-memory using a `PacketBuilder` helper defined in the test file. Methods: `.header()`, `.u8()`, `.u16()`, `.u32()`, `.u64()`, `.i8()`, `.f32()`, `.string()`, `.zeros()`, `.pad_to()`. No external fixture files.

### Naming Convention
- Standard packets: `f1_YY_<packet_type>` (e.g., `f1_23_motion_packet`)
- Changed fields: `f1_YY_<packet>_with_<feature>` (e.g., `f1_25_car_damage_with_blisters`)
- Events: `f1_YY_event_<event_type>` (e.g., `f1_24_event_safety_car`)
- Errors: descriptive snake_case (e.g., `buffer_too_short_for_body`)

### Assertion Patterns
- `assert_eq!` for integers, strings, lengths.
- `assert!((val - expected).abs() < 0.001)` for floats.
- `assert!(matches!(…))` for enum variant checks.
- `if let … else { panic!("…") }` for deep destructuring.

### Coverage
All 14 base packet types are tested for F1 23. F1 24 and F1 25 tests target additions and structural changes (new packets, new fields, updated events). Error paths (unsupported format, unknown packet ID, unknown event code, truncated buffer) are explicitly tested.
