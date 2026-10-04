use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

/// Size of the common packet header. Must match `crate::header::HEADER_SIZE`;
/// the generated code asserts this at compile time.
const HEADER_SIZE: usize = 29;

// ---------------------------------------------------------------------------
// Schema data model – deserialized from spec/*.yaml
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Spec {
    version: VersionInfo,
    /// File stem of the parent spec (e.g. `f1_2024`).
    #[serde(default)]
    extends: Option<String>,
    #[serde(default)]
    remove: Remove,
    #[serde(default)]
    structs: Vec<StructDef>,
    #[serde(default)]
    packets: Vec<PacketDef>,
    #[serde(default)]
    events: Vec<EventDef>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Remove {
    /// Struct names.
    #[serde(default)]
    structs: Vec<String>,
    /// Packet ids.
    #[serde(default)]
    packets: Vec<u8>,
    /// Event codes.
    #[serde(default)]
    events: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionInfo {
    format: u16,
    year: u8,
    module: String, // e.g. "v2023"
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct StructDef {
    name: String,
    fields: Vec<FieldDef>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldDef {
    name: String,
    ty: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct PacketDef {
    name: String,
    id: u8,
    size: usize,
    #[serde(default)]
    fields: Vec<FieldDef>,
    /// Marks the packet whose body is a 4-byte event code plus an event payload.
    #[serde(default)]
    event_packet: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct EventDef {
    name: String,
    code: String,
    #[serde(default)]
    fields: Vec<FieldDef>,
}

/// A spec with `extends`/`remove` applied: the complete definition of one game version.
#[derive(Debug)]
struct ResolvedSpec {
    version: VersionInfo,
    structs: Vec<StructDef>,
    packets: Vec<PacketDef>,
    events: Vec<EventDef>,
}

impl FieldDef {
    fn field_type(&self, module: &str, owner: &str) -> FieldType {
        FieldType::parse(&self.ty, &format!("{module}/{owner}.{}", self.name))
    }
}

// ---------------------------------------------------------------------------
// Spec inheritance
// ---------------------------------------------------------------------------

/// Replace an entry with the same key in place, or append it.
fn upsert<T, K: PartialEq>(list: &mut Vec<T>, item: T, key: impl Fn(&T) -> K) {
    let k = key(&item);
    match list.iter().position(|e| key(e) == k) {
        Some(i) => list[i] = item,
        None => list.push(item),
    }
}

/// Remove the entry with the given key; panic if none matched.
fn remove_by<T, K: PartialEq + std::fmt::Debug>(
    list: &mut Vec<T>,
    k: &K,
    key: impl Fn(&T) -> K,
    stem: &str,
    kind: &str,
) {
    let before = list.len();
    list.retain(|e| key(e) != *k);
    if list.len() == before {
        panic!("{stem}.yaml: remove.{kind} entry {k:?} matches nothing in the parent spec");
    }
}

fn resolve(stem: &str, raw: &HashMap<String, Spec>, visiting: &mut Vec<String>) -> ResolvedSpec {
    if visiting.iter().any(|s| s == stem) {
        visiting.push(stem.to_string());
        panic!("spec inheritance cycle: {}", visiting.join(" -> "));
    }
    let spec = &raw[stem];
    visiting.push(stem.to_string());

    let (mut structs, mut packets, mut events) = match &spec.extends {
        Some(parent) => {
            if !raw.contains_key(parent) {
                panic!("{stem}.yaml extends unknown spec '{parent}'");
            }
            let p = resolve(parent, raw, visiting);
            (p.structs, p.packets, p.events)
        }
        None => (Vec::new(), Vec::new(), Vec::new()),
    };
    visiting.pop();

    for name in &spec.remove.structs {
        remove_by(
            &mut structs,
            name,
            |s: &StructDef| s.name.clone(),
            stem,
            "structs",
        );
    }
    for id in &spec.remove.packets {
        remove_by(&mut packets, id, |p: &PacketDef| p.id, stem, "packets");
    }
    for code in &spec.remove.events {
        remove_by(
            &mut events,
            code,
            |e: &EventDef| e.code.clone(),
            stem,
            "events",
        );
    }

    // Duplicates inside one file would otherwise be silently collapsed by `upsert`.
    check_unique(stem, "struct", spec.structs.iter().map(|s| s.name.clone()));
    check_unique(
        stem,
        "packet id",
        spec.packets.iter().map(|p| p.id.to_string()),
    );
    check_unique(
        stem,
        "event code",
        spec.events.iter().map(|e| e.code.clone()),
    );
    for s in &spec.structs {
        upsert(&mut structs, s.clone(), |s| s.name.clone());
    }
    for p in &spec.packets {
        upsert(&mut packets, p.clone(), |p| p.id);
    }
    for e in &spec.events {
        upsert(&mut events, e.clone(), |e| e.code.clone());
    }

    ResolvedSpec {
        version: spec.version.clone(),
        structs,
        packets,
        events,
    }
}

/// Panic if any key appears twice.
fn check_unique(module: &str, kind: &str, keys: impl Iterator<Item = String>) {
    let mut seen = HashSet::new();
    for k in keys {
        if !seen.insert(k.clone()) {
            panic!("[{module}] duplicate {kind} '{k}'");
        }
    }
}

fn validate_unique(spec: &ResolvedSpec) {
    let m = &spec.version.module;
    check_unique(m, "struct", spec.structs.iter().map(|s| s.name.clone()));
    check_unique(
        m,
        "packet id",
        spec.packets.iter().map(|p| p.id.to_string()),
    );
    check_unique(
        m,
        "packet name",
        spec.packets.iter().map(|p| p.name.clone()),
    );
    check_unique(m, "event code", spec.events.iter().map(|e| e.code.clone()));
    check_unique(m, "event name", spec.events.iter().map(|e| e.name.clone()));
}

// ---------------------------------------------------------------------------
// Type representation parsed from field ty strings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum FieldType {
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    U64,
    F32,
    F64,
    PrimArray(Box<FieldType>, usize), // [prim; N]
    StructRef(String),
    StructArray(String, usize), // [Struct; N]
    String(usize),              // string(N)
}

impl FieldType {
    /// Parse a YAML type string. `ctx` (`module/Owner.field`) is used in error messages.
    fn parse(s: &str, ctx: &str) -> Self {
        let s = s.trim();
        if let Some(inner) = s.strip_prefix("string(").and_then(|r| r.strip_suffix(')')) {
            let n = inner
                .trim()
                .parse()
                .unwrap_or_else(|_| panic!("{ctx}: invalid string length in type '{s}'"));
            return Self::String(n);
        }
        if let Some(inner) = s.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            let (ty_str, count_str) = inner
                .rsplit_once(';')
                .unwrap_or_else(|| panic!("{ctx}: array must be [T; N] in type '{s}'"));
            let count: usize = count_str
                .trim()
                .parse()
                .unwrap_or_else(|_| panic!("{ctx}: invalid array count in type '{s}'"));
            let inner_ty = Self::parse(ty_str.trim(), ctx);
            if matches!(inner_ty, Self::PrimArray(..) | Self::StructArray(..)) {
                panic!("{ctx}: nested arrays are not supported in type '{s}'; wrap the inner array in a struct");
            }
            return match &inner_ty {
                Self::StructRef(name) => Self::StructArray(name.clone(), count),
                _ => Self::PrimArray(Box::new(inner_ty), count),
            };
        }
        match s {
            "u8" => Self::U8,
            "i8" => Self::I8,
            "u16" => Self::U16,
            "i16" => Self::I16,
            "u32" => Self::U32,
            "i32" => Self::I32,
            "u64" => Self::U64,
            "f32" => Self::F32,
            "f64" => Self::F64,
            other => Self::StructRef(other.to_string()),
        }
    }

    /// The Rust type string for struct field declarations.
    fn rust_type(&self) -> String {
        match self {
            Self::U8 => "u8".into(),
            Self::I8 => "i8".into(),
            Self::U16 => "u16".into(),
            Self::I16 => "i16".into(),
            Self::U32 => "u32".into(),
            Self::I32 => "i32".into(),
            Self::U64 => "u64".into(),
            Self::F32 => "f32".into(),
            Self::F64 => "f64".into(),
            Self::PrimArray(inner, n) => format!("[{}; {}]", inner.rust_type(), n),
            Self::StructRef(name) => name.clone(),
            Self::StructArray(name, n) => format!("[{}; {}]", name, n),
            Self::String(n) => format!("FixedStr<{n}>"),
        }
    }

    /// Byte size of this type; `None` if it references a struct whose size is not yet known.
    fn byte_size_opt(&self, struct_sizes: &HashMap<String, usize>) -> Option<usize> {
        match self {
            Self::U8 | Self::I8 => Some(1),
            Self::U16 | Self::I16 => Some(2),
            Self::U32 | Self::I32 | Self::F32 => Some(4),
            Self::U64 | Self::F64 => Some(8),
            Self::PrimArray(inner, n) => inner.byte_size_opt(struct_sizes).map(|s| s * n),
            Self::StructRef(name) => struct_sizes.get(name).copied(),
            Self::StructArray(name, n) => struct_sizes.get(name).map(|s| s * n),
            Self::String(n) => Some(*n),
        }
    }

    /// Byte size of this type. Panics (naming `ctx`) on an unknown struct reference.
    fn byte_size(&self, struct_sizes: &HashMap<String, usize>, ctx: &str) -> usize {
        self.byte_size_opt(struct_sizes)
            .unwrap_or_else(|| panic!("{ctx}: unknown struct in type {}", self.rust_type()))
    }

    /// Generate the reader expression for this type.
    fn reader_expr(&self) -> String {
        match self {
            Self::U8 => "reader.read_u8()?".into(),
            Self::I8 => "reader.read_i8()?".into(),
            Self::U16 => "reader.read_u16_le()?".into(),
            Self::I16 => "reader.read_i16_le()?".into(),
            Self::U32 => "reader.read_u32_le()?".into(),
            Self::I32 => "reader.read_i32_le()?".into(),
            Self::U64 => "reader.read_u64_le()?".into(),
            Self::F32 => "reader.read_f32_le()?".into(),
            Self::F64 => "reader.read_f64_le()?".into(),
            Self::PrimArray(inner, n) => match inner.as_ref() {
                Self::U8 => format!("reader.read_u8_array::<{n}>()?"),
                other => format!("reader.read_array::<{}, {n}>()?", other.rust_type()),
            },
            Self::StructRef(name) => format!("{name}::decode(reader)?"),
            Self::StructArray(name, n) => format!("reader.read_array::<{name}, {n}>()?"),
            Self::String(n) => format!("reader.read_fixed_str::<{n}>()?"),
        }
    }
}

// ---------------------------------------------------------------------------
// Size computation & validation
// ---------------------------------------------------------------------------

fn compute_struct_sizes(spec: &ResolvedSpec) -> HashMap<String, usize> {
    let module = &spec.version.module;
    let mut sizes: HashMap<String, usize> = HashMap::new();

    // Iterative resolution: structs may reference other structs in any order.
    let mut changed = true;
    while changed {
        changed = false;
        for s in &spec.structs {
            if sizes.contains_key(&s.name) {
                continue;
            }
            let mut total = 0usize;
            let mut resolved = true;
            for f in &s.fields {
                match f.field_type(module, &s.name).byte_size_opt(&sizes) {
                    Some(n) => total += n,
                    None => {
                        resolved = false;
                        break;
                    }
                }
            }
            if resolved {
                sizes.insert(s.name.clone(), total);
                changed = true;
            }
        }
    }

    for s in &spec.structs {
        if !sizes.contains_key(&s.name) {
            panic!(
                "[{module}] could not resolve size for struct '{}' — likely circular or missing dependency",
                s.name
            );
        }
    }

    sizes
}

fn validate_packet_sizes(spec: &ResolvedSpec, struct_sizes: &HashMap<String, usize>) {
    let module = &spec.version.module;
    for p in &spec.packets {
        if p.event_packet {
            // Body is a 4-byte code plus the payload; checked by `event_payload_size`.
            continue;
        }
        let mut body_size = 0usize;
        for f in &p.fields {
            let ctx = format!("{module}/{}.{}", p.name, f.name);
            body_size += f.field_type(module, &p.name).byte_size(struct_sizes, &ctx);
        }
        let total = HEADER_SIZE + body_size;
        if total != p.size {
            panic!(
                "[{module}] packet '{}' (id={}): computed size {} (header {} + body {}) != declared size {}",
                p.name, p.id, total, HEADER_SIZE, body_size, p.size
            );
        }
    }
}

/// Byte size of an event's fields.
fn event_size(module: &str, ev: &EventDef, struct_sizes: &HashMap<String, usize>) -> usize {
    ev.fields
        .iter()
        .map(|f| {
            let ctx = format!("{module}/{}.{}", ev.name, f.name);
            f.field_type(module, &ev.name).byte_size(struct_sizes, &ctx)
        })
        .sum()
}

/// Validate the event packet and every event payload; returns the payload size
/// (bytes after the 4-byte event code), or `None` if the spec has no event packet.
fn event_payload_size(spec: &ResolvedSpec, struct_sizes: &HashMap<String, usize>) -> Option<usize> {
    let module = &spec.version.module;
    let event_packets: Vec<&PacketDef> = spec.packets.iter().filter(|p| p.event_packet).collect();
    let ep = match event_packets.as_slice() {
        [] if spec.events.is_empty() => return None,
        [] => panic!("[{module}] events declared but no packet has event_packet: true"),
        [ep] => *ep,
        _ => panic!("[{module}] more than one packet has event_packet: true"),
    };
    if !ep.fields.is_empty() {
        panic!(
            "[{module}] event packet '{}' must not declare fields",
            ep.name
        );
    }
    if ep.size < HEADER_SIZE + 4 {
        panic!(
            "[{module}] event packet '{}' size {} is smaller than header + event code ({})",
            ep.name,
            ep.size,
            HEADER_SIZE + 4
        );
    }
    let payload = ep.size - HEADER_SIZE - 4;
    for ev in &spec.events {
        let valid_code = ev.code.len() == 4
            && ev
                .code
                .bytes()
                .all(|b| b.is_ascii_graphic() && b != b'"' && b != b'\\');
        if !valid_code {
            panic!(
                "[{module}] event {}: code must be 4 printable ASCII chars (no quote or backslash), got '{}'",
                ev.name, ev.code
            );
        }
        let n = event_size(module, ev, struct_sizes);
        if n > payload {
            panic!(
                "[{module}] event {} ({}) payload {n} bytes exceeds {payload}",
                ev.name, ev.code
            );
        }
    }
    Some(payload)
}

// ---------------------------------------------------------------------------
// Code generation
// ---------------------------------------------------------------------------

fn generate_module(spec: &ResolvedSpec) -> String {
    let struct_sizes = compute_struct_sizes(spec);
    validate_packet_sizes(spec, &struct_sizes);
    let payload = event_payload_size(spec, &struct_sizes);

    let mut out = String::with_capacity(64 * 1024);

    let module = &spec.version.module;
    writeln!(
        out,
        "/// Generated from `spec/*.yaml` (format {}). Do not edit.",
        spec.version.format
    )
    .unwrap();
    writeln!(out, "#[allow(unused_imports)]").unwrap();
    writeln!(out, "pub mod {module} {{").unwrap();
    writeln!(out, "    use crate::reader::{{Decode, Reader}};").unwrap();
    writeln!(out, "    use crate::error::ParseError;").unwrap();
    writeln!(out, "    use crate::fixed_str::FixedStr;").unwrap();
    writeln!(out, "    use crate::header::PacketHeader;").unwrap();
    writeln!(out, "    use super::PacketKind;").unwrap();
    writeln!(out).unwrap();

    for s in &spec.structs {
        emit_struct(&mut out, module, s);
    }
    if let Some(payload) = payload {
        emit_events(&mut out, module, &spec.events, payload, &struct_sizes);
    }
    for p in &spec.packets {
        emit_packet_struct(&mut out, module, p);
    }
    emit_packet_enum(&mut out, &spec.packets, spec.version.format);

    writeln!(out, "}}").unwrap();
    out
}

fn emit_struct(out: &mut String, module: &str, s: &StructDef) {
    writeln!(out, "    #[derive(Debug, Clone, Copy, PartialEq, Default)]").unwrap();
    writeln!(out, "    pub struct {} {{", s.name).unwrap();
    for f in &s.fields {
        let ft = f.field_type(module, &s.name);
        writeln!(out, "        pub {}: {},", f.name, ft.rust_type()).unwrap();
    }
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(out, "    impl Decode for {} {{", s.name).unwrap();
    writeln!(
        out,
        "        fn decode(reader: &mut Reader<'_>) -> Result<Self, ParseError> {{"
    )
    .unwrap();
    writeln!(out, "            Ok(Self {{").unwrap();
    for f in &s.fields {
        let ft = f.field_type(module, &s.name);
        writeln!(out, "                {}: {},", f.name, ft.reader_expr()).unwrap();
    }
    writeln!(out, "            }})").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
}

fn emit_events(
    out: &mut String,
    module: &str,
    events: &[EventDef],
    payload: usize,
    struct_sizes: &HashMap<String, usize>,
) {
    writeln!(
        out,
        "    /// Event payload, selected by the 4-byte event code."
    )
    .unwrap();
    writeln!(out, "    #[non_exhaustive]").unwrap();
    writeln!(out, "    #[derive(Debug, Clone, Copy, PartialEq)]").unwrap();
    writeln!(out, "    pub enum EventDetails {{").unwrap();
    for ev in events {
        if ev.fields.is_empty() {
            writeln!(out, "        {},", ev.name).unwrap();
        } else {
            writeln!(out, "        {} {{", ev.name).unwrap();
            for f in &ev.fields {
                let ft = f.field_type(module, &ev.name);
                writeln!(out, "            {}: {},", f.name, ft.rust_type()).unwrap();
            }
            writeln!(out, "        }},").unwrap();
        }
    }
    writeln!(
        out,
        "        /// An event code this crate does not know; the raw payload is preserved."
    )
    .unwrap();
    writeln!(
        out,
        "        Unknown {{ code: [u8; 4], payload: [u8; {payload}] }},"
    )
    .unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(out, "    impl EventDetails {{").unwrap();
    writeln!(
        out,
        "        /// Decode the payload for `code`; always consumes exactly {payload} bytes."
    )
    .unwrap();
    writeln!(
        out,
        "        pub fn decode(code: &[u8; 4], reader: &mut Reader<'_>) -> Result<Self, ParseError> {{"
    )
    .unwrap();
    writeln!(out, "            let details = match code {{").unwrap();
    for ev in events {
        let code = &ev.code;
        let padding = payload - event_size(module, ev, struct_sizes);
        writeln!(out, "                b\"{code}\" => {{").unwrap();
        // With padding, bind the value, skip the unused payload bytes, then yield it.
        let (open, close) = if padding > 0 {
            ("let v = ", ";")
        } else {
            ("", "")
        };
        if ev.fields.is_empty() {
            writeln!(out, "                    {open}Self::{}{close}", ev.name).unwrap();
        } else {
            writeln!(out, "                    {open}Self::{} {{", ev.name).unwrap();
            for f in &ev.fields {
                let ft = f.field_type(module, &ev.name);
                writeln!(
                    out,
                    "                        {}: {},",
                    f.name,
                    ft.reader_expr()
                )
                .unwrap();
            }
            writeln!(out, "                    }}{close}").unwrap();
        }
        if padding > 0 {
            writeln!(out, "                    reader.skip({padding})?;").unwrap();
            writeln!(out, "                    v").unwrap();
        }
        writeln!(out, "                }}").unwrap();
    }
    writeln!(
        out,
        "                other => Self::Unknown {{ code: *other, payload: reader.read_u8_array::<{payload}>()? }},"
    )
    .unwrap();
    writeln!(out, "            }};").unwrap();
    writeln!(out, "            Ok(details)").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
}

fn emit_packet_struct(out: &mut String, module: &str, p: &PacketDef) {
    let struct_name = format!("Packet{}", p.name);
    writeln!(out, "    #[derive(Debug, Clone, PartialEq)]").unwrap();
    writeln!(out, "    pub struct {struct_name} {{").unwrap();
    writeln!(out, "        pub header: PacketHeader,").unwrap();
    if p.event_packet {
        writeln!(out, "        pub event_code: [u8; 4],").unwrap();
        writeln!(out, "        pub event_details: EventDetails,").unwrap();
    } else {
        for f in &p.fields {
            let ft = f.field_type(module, &p.name);
            writeln!(out, "        pub {}: {},", f.name, ft.rust_type()).unwrap();
        }
    }
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(out, "    impl {struct_name} {{").unwrap();
    writeln!(
        out,
        "        /// Decode the packet body; `reader` must be positioned just after the header."
    )
    .unwrap();
    writeln!(
        out,
        "        pub fn decode_body(header: PacketHeader, reader: &mut Reader<'_>) -> Result<Self, ParseError> {{"
    )
    .unwrap();
    if p.event_packet {
        writeln!(
            out,
            "            let event_code = reader.read_u8_array::<4>()?;"
        )
        .unwrap();
        writeln!(
            out,
            "            let event_details = EventDetails::decode(&event_code, reader)?;"
        )
        .unwrap();
        writeln!(
            out,
            "            Ok(Self {{ header, event_code, event_details }})"
        )
        .unwrap();
    } else {
        writeln!(out, "            Ok(Self {{").unwrap();
        writeln!(out, "                header,").unwrap();
        for f in &p.fields {
            let ft = f.field_type(module, &p.name);
            writeln!(out, "                {}: {},", f.name, ft.reader_expr()).unwrap();
        }
        writeln!(out, "            }})").unwrap();
    }
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
}

fn emit_packet_enum(out: &mut String, packets: &[PacketDef], format: u16) {
    writeln!(out, "    #[non_exhaustive]").unwrap();
    writeln!(out, "    #[derive(Debug, Clone, PartialEq)]").unwrap();
    writeln!(out, "    #[allow(clippy::large_enum_variant)]").unwrap();
    writeln!(out, "    pub enum Packet {{").unwrap();
    for p in packets {
        writeln!(out, "        {}(Packet{}),", p.name, p.name).unwrap();
    }
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(out, "    impl Packet {{").unwrap();
    writeln!(
        out,
        "        /// Decode a full datagram (header + body) for this game version."
    )
    .unwrap();
    writeln!(
        out,
        "        pub fn decode(reader: &mut Reader<'_>) -> Result<Self, ParseError> {{"
    )
    .unwrap();
    writeln!(
        out,
        "            let header = PacketHeader::decode(reader)?;"
    )
    .unwrap();
    writeln!(out, "            let total_len = reader.len();").unwrap();
    writeln!(out, "            match header.packet_id {{").unwrap();
    for p in packets {
        writeln!(out, "                {} => {{", p.id).unwrap();
        writeln!(out, "                    if total_len < {} {{", p.size).unwrap();
        writeln!(
            out,
            "                        return Err(ParseError::SizeMismatch {{ expected: {}, actual: total_len }});",
            p.size
        )
        .unwrap();
        writeln!(out, "                    }}").unwrap();
        writeln!(
            out,
            "                    Ok(Self::{0}(Packet{0}::decode_body(header, reader)?))",
            p.name
        )
        .unwrap();
        writeln!(out, "                }}").unwrap();
    }
    writeln!(
        out,
        "                other => Err(ParseError::UnknownPacketId {{ packet_id: other, format: {format} }}),"
    )
    .unwrap();
    writeln!(out, "            }}").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out).unwrap();

    writeln!(out, "        /// The packet header.").unwrap();
    writeln!(out, "        pub fn header(&self) -> &PacketHeader {{").unwrap();
    writeln!(out, "            match self {{").unwrap();
    for p in packets {
        writeln!(out, "                Self::{}(p) => &p.header,", p.name).unwrap();
    }
    writeln!(out, "            }}").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out).unwrap();

    writeln!(out, "        /// The packet kind.").unwrap();
    writeln!(out, "        pub fn kind(&self) -> PacketKind {{").unwrap();
    writeln!(out, "            match self {{").unwrap();
    for p in packets {
        writeln!(
            out,
            "                Self::{0}(_) => PacketKind::{0},",
            p.name
        )
        .unwrap();
    }
    writeln!(out, "            }}").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
}

/// Emit the crate-level items that span all versions: `GameVersion`,
/// `PacketKind` and the top-level `Packet` enum with format dispatch.
fn emit_crate_items(out: &mut String, specs: &[ResolvedSpec]) {
    let variant = |v: &VersionInfo| format!("F1_{:02}", v.year);

    writeln!(out, "use crate::error::ParseError;").unwrap();
    writeln!(out, "use crate::header::PacketHeader;").unwrap();
    writeln!(out, "use crate::reader::Reader;").unwrap();
    writeln!(out).unwrap();

    // GameVersion
    writeln!(
        out,
        "/// Supported game versions, in ascending `packet_format` order."
    )
    .unwrap();
    writeln!(out, "#[non_exhaustive]").unwrap();
    writeln!(out, "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]").unwrap();
    writeln!(out, "pub enum GameVersion {{").unwrap();
    for s in specs {
        writeln!(out, "    {},", variant(&s.version)).unwrap();
    }
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "impl GameVersion {{").unwrap();
    writeln!(
        out,
        "    /// Resolve from the `packet_format` field in the header."
    )
    .unwrap();
    writeln!(
        out,
        "    pub fn from_format(format: u16) -> Option<Self> {{"
    )
    .unwrap();
    writeln!(out, "        match format {{").unwrap();
    for s in specs {
        writeln!(
            out,
            "            {} => Some(Self::{}),",
            s.version.format,
            variant(&s.version)
        )
        .unwrap();
    }
    writeln!(out, "            _ => None,").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "    /// The `packet_format` value for this version.").unwrap();
    writeln!(out, "    pub fn format(self) -> u16 {{").unwrap();
    writeln!(out, "        match self {{").unwrap();
    for s in specs {
        writeln!(
            out,
            "            Self::{} => {},",
            variant(&s.version),
            s.version.format
        )
        .unwrap();
    }
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "    /// The `game_year` byte value (last two digits).").unwrap();
    writeln!(out, "    pub fn year(self) -> u8 {{").unwrap();
    writeln!(out, "        match self {{").unwrap();
    for s in specs {
        writeln!(
            out,
            "            Self::{} => {},",
            variant(&s.version),
            s.version.year
        )
        .unwrap();
    }
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "impl std::fmt::Display for GameVersion {{").unwrap();
    writeln!(
        out,
        "    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {{"
    )
    .unwrap();
    writeln!(out, "        write!(f, \"F1 {{:02}}\", self.year())").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    // PacketKind: union of packet (id, name) across all versions.
    let mut kinds: BTreeMap<u8, (String, Vec<String>)> = BTreeMap::new();
    let mut name_ids: HashMap<String, u8> = HashMap::new();
    for s in specs {
        let label = format!("F1 {:02}", s.version.year);
        for p in &s.packets {
            if let Some(&id) = name_ids.get(&p.name) {
                if id != p.id {
                    panic!(
                        "[{}] packet '{}' has id {} but id {id} in an earlier spec",
                        s.version.module, p.name, p.id
                    );
                }
            }
            name_ids.insert(p.name.clone(), p.id);
            let entry = kinds
                .entry(p.id)
                .or_insert_with(|| (p.name.clone(), Vec::new()));
            if entry.0 != p.name {
                panic!(
                    "[{}] packet id {} is '{}' but '{}' in an earlier spec",
                    s.version.module, p.id, p.name, entry.0
                );
            }
            entry.1.push(label.clone());
        }
    }
    writeln!(out, "/// Packet kinds across all supported versions.").unwrap();
    writeln!(out, "#[non_exhaustive]").unwrap();
    writeln!(out, "#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]").unwrap();
    writeln!(out, "pub enum PacketKind {{").unwrap();
    for (name, versions) in kinds.values() {
        writeln!(out, "    /// Present in: {}.", versions.join(", ")).unwrap();
        writeln!(out, "    {name},").unwrap();
    }
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "impl PacketKind {{").unwrap();
    writeln!(out, "    /// Resolve from a raw packet id byte.").unwrap();
    writeln!(out, "    pub fn from_id(id: u8) -> Option<Self> {{").unwrap();
    writeln!(out, "        match id {{").unwrap();
    for (id, (name, _)) in &kinds {
        writeln!(out, "            {id} => Some(Self::{name}),").unwrap();
    }
    writeln!(out, "            _ => None,").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "    /// The raw packet id for this kind.").unwrap();
    writeln!(out, "    pub fn id(self) -> u8 {{").unwrap();
    writeln!(out, "        match self {{").unwrap();
    for (id, (name, _)) in &kinds {
        writeln!(out, "            Self::{name} => {id},").unwrap();
    }
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();

    // Top-level Packet
    writeln!(
        out,
        "/// A parsed F1 telemetry packet, tagged by game version."
    )
    .unwrap();
    writeln!(out, "#[non_exhaustive]").unwrap();
    writeln!(out, "#[derive(Debug, Clone, PartialEq)]").unwrap();
    writeln!(out, "#[allow(clippy::large_enum_variant)]").unwrap();
    writeln!(out, "pub enum Packet {{").unwrap();
    for s in specs {
        writeln!(
            out,
            "    {}({}::Packet),",
            variant(&s.version),
            s.version.module
        )
        .unwrap();
    }
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "impl Packet {{").unwrap();
    writeln!(out, "    /// Read `packet_format` without consuming it, then decode with that version's layout.").unwrap();
    writeln!(
        out,
        "    pub fn decode(reader: &mut Reader<'_>) -> Result<Self, ParseError> {{"
    )
    .unwrap();
    writeln!(out, "        match reader.peek_u16_le()? {{").unwrap();
    for s in specs {
        writeln!(
            out,
            "            {} => Ok(Self::{}({}::Packet::decode(reader)?)),",
            s.version.format,
            variant(&s.version),
            s.version.module
        )
        .unwrap();
    }
    writeln!(
        out,
        "            other => Err(ParseError::UnsupportedFormat(other)),"
    )
    .unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
    for (doc, method, ret, arm) in [
        (
            "The packet header.",
            "header",
            "&PacketHeader",
            "p.header()",
        ),
        ("The packet kind.", "kind", "PacketKind", "p.kind()"),
    ] {
        writeln!(out, "    /// {doc}").unwrap();
        writeln!(out, "    pub fn {method}(&self) -> {ret} {{").unwrap();
        writeln!(out, "        match self {{").unwrap();
        for s in specs {
            writeln!(
                out,
                "            Self::{}(p) => {arm},",
                variant(&s.version)
            )
            .unwrap();
        }
        writeln!(out, "        }}").unwrap();
        writeln!(out, "    }}").unwrap();
        writeln!(out).unwrap();
    }
    writeln!(out, "    /// The game version this packet came from.").unwrap();
    writeln!(out, "    pub fn version(&self) -> GameVersion {{").unwrap();
    writeln!(out, "        match self {{").unwrap();
    for s in specs {
        writeln!(
            out,
            "            Self::{0}(_) => GameVersion::{0},",
            variant(&s.version)
        )
        .unwrap();
    }
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();
}

// ---------------------------------------------------------------------------
// Main build script
// ---------------------------------------------------------------------------

/// Load and resolve every `spec/*.yaml`, sorted by `packet_format`.
fn load_specs(spec_dir: &Path) -> Vec<ResolvedSpec> {
    println!("cargo::rerun-if-changed={}", spec_dir.display());

    let mut raw: HashMap<String, Spec> = HashMap::new();
    let entries = fs::read_dir(spec_dir)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", spec_dir.display()));
    for entry in entries {
        let path = entry
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", spec_dir.display()))
            .path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        println!("cargo::rerun-if-changed={}", path.display());
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_else(|| panic!("non-UTF-8 spec file name: {}", path.display()))
            .to_string();
        let yaml = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        let spec: Spec = serde_yaml::from_str(&yaml)
            .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()));
        raw.insert(stem, spec);
    }

    let mut specs: Vec<ResolvedSpec> = raw
        .keys()
        .map(|stem| resolve(stem, &raw, &mut Vec::new()))
        .collect();
    specs.sort_by_key(|s| s.version.format);

    check_unique(
        "spec",
        "format",
        specs.iter().map(|s| s.version.format.to_string()),
    );
    check_unique(
        "spec",
        "module",
        specs.iter().map(|s| s.version.module.clone()),
    );
    check_unique(
        "spec",
        "year",
        specs.iter().map(|s| s.version.year.to_string()),
    );
    for s in &specs {
        validate_unique(s);
    }
    specs
}

fn main() {
    println!("cargo::rerun-if-changed=build.rs");

    let specs = load_specs(Path::new("spec"));

    let mut all_modules = String::new();
    writeln!(
        all_modules,
        "const _: () = assert!(crate::header::HEADER_SIZE == {HEADER_SIZE});\n"
    )
    .unwrap();
    for spec in &specs {
        all_modules.push_str(&generate_module(spec));
        all_modules.push('\n');
    }
    emit_crate_items(&mut all_modules, &specs);

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let dest = Path::new(&out_dir).join("generated.rs");
    fs::write(&dest, &all_modules)
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", dest.display()));
}
