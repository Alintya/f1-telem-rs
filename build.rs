use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
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
    /// Named array lengths, usable in `[T; N]` counts (e.g. `cars: 22`). Inherited;
    /// a child overriding a constant also resizes the arrays it inherits.
    #[serde(default)]
    consts: BTreeMap<String, usize>,
    /// Declared-size overrides for inherited packets, keyed by packet id. Needed when a
    /// constant or struct change resizes a packet the child does not restate.
    #[serde(default)]
    sizes: BTreeMap<u8, usize>,
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
    /// Quantisation factor for integer fields: physical value = raw * scale.
    #[serde(default)]
    scale: Option<f64>,
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
    consts: BTreeMap<String, usize>,
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

    let (mut consts, mut structs, mut packets, mut events) = match &spec.extends {
        Some(parent) => {
            if !raw.contains_key(parent) {
                panic!("{stem}.yaml extends unknown spec '{parent}'");
            }
            let p = resolve(parent, raw, visiting);
            (p.consts, p.structs, p.packets, p.events)
        }
        None => (BTreeMap::new(), Vec::new(), Vec::new(), Vec::new()),
    };
    visiting.pop();
    consts.extend(spec.consts.iter().map(|(k, v)| (k.clone(), *v)));

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

    for (id, size) in &spec.sizes {
        if spec.packets.iter().any(|p| p.id == *id) {
            panic!("{stem}.yaml: packet {id} is restated; set its size there, not in `sizes`");
        }
        match packets.iter_mut().find(|p| p.id == *id) {
            Some(p) => p.size = *size,
            None => panic!("{stem}.yaml: sizes entry for unknown packet id {id}"),
        }
    }

    ResolvedSpec {
        version: spec.version.clone(),
        consts,
        structs,
        packets,
        events,
    }
}

/// Replace named constants in array counts (`[T; cars]`, `[u8; 50 * cars]`) with numbers.
/// Runs after the whole `extends` chain is resolved so child overrides apply everywhere.
fn apply_consts(spec: &mut ResolvedSpec) {
    let module = spec.version.module.clone();
    let consts = &spec.consts;
    let expand = |owner: &str, f: &mut FieldDef| {
        let ty = f.ty.trim();
        let Some((inner, count)) = ty
            .strip_prefix('[')
            .and_then(|r| r.strip_suffix(']'))
            .and_then(|r| r.rsplit_once(';'))
        else {
            return;
        };
        let ctx = format!("{module}/{owner}.{}", f.name);
        let n: usize = count
            .split('*')
            .map(|term| {
                let term = term.trim();
                term.parse::<usize>().unwrap_or_else(|_| {
                    *consts.get(term).unwrap_or_else(|| {
                        panic!("{ctx}: unknown constant '{term}' in type '{ty}'")
                    })
                })
            })
            .product();
        f.ty = format!("[{}; {n}]", inner.trim());
    };
    for s in &mut spec.structs {
        for f in &mut s.fields {
            expand(&s.name, f);
        }
    }
    for p in &mut spec.packets {
        for f in &mut p.fields {
            expand(&p.name, f);
        }
    }
    for e in &mut spec.events {
        for f in &mut e.fields {
            expand(&e.name, f);
        }
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

    // `scale` marks a quantised integer: physical value = raw * scale.
    let owners = spec
        .structs
        .iter()
        .map(|s| (&s.name, &s.fields))
        .chain(spec.packets.iter().map(|p| (&p.name, &p.fields)))
        .chain(spec.events.iter().map(|e| (&e.name, &e.fields)));
    for (owner, fields) in owners {
        for f in fields.iter().filter(|f| f.scale.is_some()) {
            let scale = f.scale.unwrap_or_default();
            let integer = matches!(
                f.field_type(m, owner),
                FieldType::U8
                    | FieldType::I8
                    | FieldType::U16
                    | FieldType::I16
                    | FieldType::U32
                    | FieldType::I32
            );
            if !integer || !scale.is_finite() || scale <= 0.0 {
                panic!(
                    "{m}/{owner}.{}: `scale` needs a positive finite value on an integer field up to 32 bits",
                    f.name
                );
            }
        }
    }
}

/// Doc comment line for a generated field, if it carries one.
fn field_doc(f: &FieldDef, indent: &str) -> String {
    match f.scale {
        Some(scale) => format!(
            "{indent}/// Quantised: physical value = raw {}.\n",
            scale_op(scale)
        ),
        None => String::new(),
    }
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
        if sizes.contains_key(&s.name) {
            continue;
        }
        // Prefer naming a reference to a struct that does not exist at all.
        for f in &s.fields {
            if let FieldType::StructRef(name) | FieldType::StructArray(name, _) =
                f.field_type(module, &s.name)
            {
                if !spec.structs.iter().any(|d| d.name == name) {
                    panic!("{module}/{}.{}: unknown struct '{name}'", s.name, f.name);
                }
            }
        }
        panic!(
            "[{module}] could not resolve size for struct '{}' — circular dependency",
            s.name
        );
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
        out.push_str(&field_doc(f, "        "));
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
                out.push_str(&field_doc(f, "            "));
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
            out.push_str(&field_doc(f, "        "));
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
    writeln!(out, "///").unwrap();
    writeln!(
        out,
        "/// Kinds introduced in later years still exist here; a datagram whose packet id"
    )
    .unwrap();
    writeln!(
        out,
        "/// its version does not declare fails with `ParseError::UnknownPacketId`."
    )
    .unwrap();
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
    writeln!(
        out,
        "    /// Call the matching [`common::PacketVisitor`] method with this packet."
    )
    .unwrap();
    writeln!(
        out,
        "    pub fn visit<V: common::PacketVisitor>(&self, visitor: &mut V) {{"
    )
    .unwrap();
    writeln!(out, "        match self {{").unwrap();
    for s in specs {
        writeln!(
            out,
            "            Self::{}(p) => p.visit(visitor),",
            variant(&s.version)
        )
        .unwrap();
    }
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
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
// Cross-version view (`common` module)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum Num {
    Int { signed: bool, bits: u32 },
    Float { bits: u32 },
}

impl Num {
    fn of(ft: &FieldType) -> Option<Self> {
        Some(match ft {
            FieldType::U8 => Self::Int {
                signed: false,
                bits: 8,
            },
            FieldType::I8 => Self::Int {
                signed: true,
                bits: 8,
            },
            FieldType::U16 => Self::Int {
                signed: false,
                bits: 16,
            },
            FieldType::I16 => Self::Int {
                signed: true,
                bits: 16,
            },
            FieldType::U32 => Self::Int {
                signed: false,
                bits: 32,
            },
            FieldType::I32 => Self::Int {
                signed: true,
                bits: 32,
            },
            FieldType::U64 => Self::Int {
                signed: false,
                bits: 64,
            },
            FieldType::F32 => Self::Float { bits: 32 },
            FieldType::F64 => Self::Float { bits: 64 },
            _ => return None,
        })
    }

    fn rust(self) -> String {
        match self {
            Self::Int { signed, bits } => format!("{}{bits}", if signed { 'i' } else { 'u' }),
            Self::Float { bits } => format!("f{bits}"),
        }
    }

    /// Whether every value of integer type `src` fits in integer type `self`.
    fn holds(self, src: Self) -> bool {
        match (self, src) {
            (
                Self::Int {
                    signed: ts,
                    bits: tb,
                },
                Self::Int {
                    signed: ss,
                    bits: sb,
                },
            ) => match (ts, ss) {
                (false, false) | (true, true) => sb <= tb,
                (true, false) => sb < tb,
                (false, true) => false,
            },
            _ => false,
        }
    }
}

/// Type of one field in the cross-version view.
#[derive(Debug, Clone)]
enum CommonTy {
    Num(Num),
    Str,
    Array { elem: String, len: Option<usize> },
    Struct(String),
    StructArray { name: String, len: Option<usize> },
}

impl CommonTy {
    /// Accessor return type (`len: None` means lengths differ across versions → slice).
    fn ret(&self) -> String {
        match self {
            Self::Num(n) => n.rust(),
            Self::Str => "std::borrow::Cow<'_, str>".into(),
            Self::Array { elem, len: Some(n) } => format!("&[{elem}; {n}]"),
            Self::Array { elem, len: None } => format!("&[{elem}]"),
            Self::Struct(name) => format!("&Self::{name}"),
            Self::StructArray { name, len: Some(n) } => format!("&[Self::{name}; {n}]"),
            Self::StructArray { name, len: None } => format!("&[Self::{name}]"),
        }
    }

    /// Expression converting one version's value `expr` (of type `ft`) to this type.
    fn convert(&self, expr: &str, ft: &FieldType, scale: Option<f64>) -> String {
        match self {
            Self::Num(target) => {
                let src = Num::of(ft).expect("unify only yields Num for numeric fields");
                let t = target.rust();
                match (scale, *target) {
                    (Some(s), _) => format!("({expr} as {t}) {}", scale_op(s)),
                    (None, _) if src == *target => expr.to_string(),
                    (None, Num::Float { .. }) if matches!(src, Num::Float { .. }) => {
                        format!("{t}::from({expr})")
                    }
                    (None, Num::Float { .. }) => format!("{expr} as {t}"),
                    (None, Num::Int { .. }) => format!("{t}::from({expr})"),
                }
            }
            Self::Str => format!("{expr}.to_string_lossy()"),
            Self::Array { len: None, .. } | Self::StructArray { len: None, .. } => {
                format!("&{expr}[..]")
            }
            _ => format!("&{expr}"),
        }
    }
}

/// The arithmetic applying a quantisation factor: `/ 1000.0` when the factor is the
/// reciprocal of an integer (matching EA's "divide by N" wording exactly, since e.g.
/// 0.001 is not representable in binary), otherwise `* factor`.
fn scale_op(scale: f64) -> String {
    let divisor = (1.0 / scale).round();
    if divisor > 1.0 && (divisor * scale - 1.0).abs() < 1e-12 {
        format!("/ {divisor:?}")
    } else {
        format!("* {scale:?}")
    }
}

/// One version's definition of a field: (spec index, type, scale).
type Occurrence = (usize, FieldType, Option<f64>);

/// The field lists one owner (struct, packet or event) has in each version: (spec index, fields).
type VersionFields<'a> = Vec<(usize, &'a [FieldDef])>;

/// The common type of a field across versions, or `None` if the definitions
/// cannot be reconciled; such fields are left out of the common view.
fn unify(occ: &[Occurrence]) -> Option<CommonTy> {
    let same = |lens: &[usize]| lens.iter().all(|l| *l == lens[0]).then_some(lens[0]);
    match &occ[0].1 {
        FieldType::String(_) => occ
            .iter()
            .all(|o| matches!(o.1, FieldType::String(_)))
            .then_some(CommonTy::Str),
        FieldType::StructRef(name) => occ
            .iter()
            .all(|o| matches!(&o.1, FieldType::StructRef(n) if n == name))
            .then(|| CommonTy::Struct(name.clone())),
        FieldType::StructArray(name, _) => {
            let lens = occ
                .iter()
                .map(|o| match &o.1 {
                    FieldType::StructArray(n, l) if n == name => Some(*l),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()?;
            Some(CommonTy::StructArray {
                name: name.clone(),
                len: same(&lens),
            })
        }
        FieldType::PrimArray(elem, _) => {
            let elem = elem.rust_type();
            let lens = occ
                .iter()
                .map(|o| match &o.1 {
                    FieldType::PrimArray(e, l) if e.rust_type() == elem => Some(*l),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()?;
            Some(CommonTy::Array {
                elem,
                len: same(&lens),
            })
        }
        _ => {
            let nums = occ
                .iter()
                .map(|o| Num::of(&o.1))
                .collect::<Option<Vec<_>>>()?;
            let scaled = occ.iter().any(|o| o.2.is_some());
            if !scaled && nums.iter().all(|n| *n == nums[0]) {
                return Some(CommonTy::Num(nums[0]));
            }
            if scaled || nums.iter().any(|n| matches!(n, Num::Float { .. })) {
                let wide = nums
                    .iter()
                    .any(|n| matches!(n, Num::Float { bits: 64 } | Num::Int { bits: 32.., .. }));
                return Some(CommonTy::Num(Num::Float {
                    bits: if wide { 64 } else { 32 },
                }));
            }
            [8, 16, 32, 64]
                .into_iter()
                .flat_map(|bits| [false, true].map(|signed| Num::Int { signed, bits }))
                .find(|t| nums.iter().all(|n| t.holds(*n)))
                .map(CommonTy::Num)
        }
    }
}

struct CommonField {
    name: String,
    ty: CommonTy,
    occ: Vec<Occurrence>,
    /// Missing in at least one version that defines the owner.
    optional: bool,
}

/// Union of an owner's fields over the versions that define it, in first-seen order,
/// plus the names of fields left out because `accept` rejects them or their types
/// cannot be reconciled. Each omission is reported as a cargo warning.
fn common_fields(
    specs: &[ResolvedSpec],
    owner: &str,
    defs: &[(usize, &[FieldDef])],
    accept: impl Fn(&CommonTy) -> bool,
) -> (Vec<CommonField>, Vec<String>) {
    let mut merged: Vec<(String, Vec<Occurrence>)> = Vec::new();
    for (si, fields) in defs {
        let module = &specs[*si].version.module;
        for f in *fields {
            let o = (*si, f.field_type(module, owner), f.scale);
            match merged.iter_mut().find(|(n, _)| *n == f.name) {
                Some((_, occ)) => occ.push(o),
                None => merged.push((f.name.clone(), vec![o])),
            }
        }
    }
    let mut kept = Vec::new();
    let mut omitted = Vec::new();
    for (name, occ) in merged {
        match unify(&occ).filter(&accept) {
            Some(ty) => kept.push(CommonField {
                optional: occ.len() < defs.len(),
                name,
                ty,
                occ,
            }),
            None => {
                println!(
                    "cargo::warning=common::{owner}.{name} omitted: its types differ across versions; use the version-specific types"
                );
                omitted.push(name);
            }
        }
    }
    (kept, omitted)
}

/// Doc line listing fields only available on the version-specific types.
fn omitted_doc(indent: &str, omitted: &[String]) -> String {
    if omitted.is_empty() {
        return String::new();
    }
    let names: Vec<String> = omitted.iter().map(|n| format!("`{n}`")).collect();
    format!(
        "{indent}///\n{indent}/// Not exposed here because their types differ across versions (use the\n{indent}/// version-specific types): {}.\n",
        names.join(", ")
    )
}

fn version_label(v: &VersionInfo) -> String {
    format!("F1 {:02}", v.year)
}

fn present_in(specs: &[ResolvedSpec], indices: impl Iterator<Item = usize>) -> String {
    indices
        .map(|si| version_label(&specs[si].version))
        .collect::<Vec<_>>()
        .join(", ")
}

fn snake_case(name: &str) -> String {
    let mut s = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                s.push('_');
            }
            s.push(c.to_ascii_lowercase());
        } else {
            s.push(c);
        }
    }
    s
}

/// Emit a trait named `concrete` over the field union of `owner`, and its
/// impls for each version's `concrete` type (plus `Absent` for structs).
fn emit_common_trait(
    out: &mut String,
    specs: &[ResolvedSpec],
    owner: &str,
    concrete: &str,
    defs: &[(usize, &[FieldDef])],
    packet: bool,
) {
    let (fields, omitted) = common_fields(specs, owner, defs, |_| true);
    let assoc: BTreeSet<&str> = fields
        .iter()
        .filter_map(|f| match &f.ty {
            CommonTy::Struct(n) | CommonTy::StructArray { name: n, .. } => Some(n.as_str()),
            _ => None,
        })
        .collect();
    let ret = |f: &CommonField| {
        let r = f.ty.ret();
        if f.optional {
            format!("Option<{r}>")
        } else {
            r
        }
    };

    writeln!(
        out,
        "    /// `{concrete}` across versions. Implemented in: {}.",
        present_in(specs, defs.iter().map(|d| d.0))
    )
    .unwrap();
    out.push_str(&omitted_doc("    ", &omitted));
    writeln!(out, "    pub trait {concrete} {{").unwrap();
    for a in &assoc {
        writeln!(out, "        type {a}: {a};").unwrap();
    }
    if packet {
        writeln!(out, "        /// The packet header.").unwrap();
        writeln!(out, "        fn header(&self) -> &PacketHeader;").unwrap();
    }
    for f in &fields {
        if f.optional {
            writeln!(
                out,
                "        /// `None` in versions without this field. Present in: {}.",
                present_in(specs, f.occ.iter().map(|o| o.0))
            )
            .unwrap();
        }
        if f.occ.iter().any(|o| o.2.is_some()) {
            writeln!(
                out,
                "        /// Physical value; quantised versions are scaled."
            )
            .unwrap();
        }
        writeln!(out, "        fn {}(&self) -> {};", f.name, ret(f)).unwrap();
    }
    writeln!(out, "    }}\n").unwrap();

    for (si, _) in defs {
        let spec = &specs[*si];
        let m = &spec.version.module;
        writeln!(out, "    impl {concrete} for super::{m}::{concrete} {{").unwrap();
        for a in &assoc {
            let target = if spec.structs.iter().any(|s| s.name == *a) {
                format!("super::{m}::{a}")
            } else {
                "Absent".into()
            };
            writeln!(out, "        type {a} = {target};").unwrap();
        }
        if packet {
            writeln!(
                out,
                "        fn header(&self) -> &PacketHeader {{ &self.header }}"
            )
            .unwrap();
        }
        for f in &fields {
            let body = match f.occ.iter().find(|o| o.0 == *si) {
                Some((_, ft, scale)) => {
                    let e = f.ty.convert(&format!("self.{}", f.name), ft, *scale);
                    if f.optional {
                        format!("Some({e})")
                    } else {
                        e
                    }
                }
                None => "None".into(),
            };
            writeln!(
                out,
                "        fn {}(&self) -> {} {{ {body} }}",
                f.name,
                ret(f)
            )
            .unwrap();
        }
        writeln!(out, "    }}\n").unwrap();
    }

    if !packet {
        writeln!(out, "    impl {concrete} for Absent {{").unwrap();
        for a in &assoc {
            writeln!(out, "        type {a} = Absent;").unwrap();
        }
        for f in &fields {
            writeln!(
                out,
                "        fn {}(&self) -> {} {{ match *self {{}} }}",
                f.name,
                ret(f)
            )
            .unwrap();
        }
        writeln!(out, "    }}\n").unwrap();
    }
}

/// Emit `common::EventDetails` (union of every version's events) and the
/// `From<&vXXXX::EventDetails>` conversions.
fn emit_common_events(out: &mut String, specs: &[ResolvedSpec]) {
    struct Ev<'a> {
        code: String,
        name: String,
        defs: Vec<(usize, &'a EventDef)>,
    }
    let mut evs: Vec<Ev> = Vec::new();
    for (si, s) in specs.iter().enumerate() {
        for e in &s.events {
            match evs.iter_mut().find(|x| x.code == e.code) {
                Some(x) if x.name != e.name => panic!(
                    "[{}] event code {} is '{}' but '{}' in an earlier spec",
                    s.version.module, e.code, e.name, x.name
                ),
                Some(x) => x.defs.push((si, e)),
                None => {
                    if let Some(x) = evs.iter().find(|x| x.name == e.name) {
                        panic!(
                            "[{}] event '{}' has code {} but {} in an earlier spec",
                            s.version.module, e.name, e.code, x.code
                        );
                    }
                    evs.push(Ev {
                        code: e.code.clone(),
                        name: e.name.clone(),
                        defs: vec![(si, e)],
                    });
                }
            }
        }
    }
    let payloads: Vec<(usize, usize)> = specs
        .iter()
        .enumerate()
        .filter_map(|(si, s)| event_payload_size(s, &compute_struct_sizes(s)).map(|p| (si, p)))
        .collect();
    let Some(max_payload) = payloads.iter().map(|p| p.1).max() else {
        return;
    };
    // Only scalar event fields are representable in a `Copy` enum shared across versions.
    let (fields, omitted): (Vec<Vec<CommonField>>, Vec<Vec<String>>) = evs
        .iter()
        .map(|ev| {
            let defs: Vec<(usize, &[FieldDef])> =
                ev.defs.iter().map(|(si, e)| (*si, &e.fields[..])).collect();
            common_fields(specs, &ev.name, &defs, |ty| matches!(ty, CommonTy::Num(_)))
        })
        .unzip();

    writeln!(
        out,
        "    /// Event payload across versions; convert with `EventDetails::from(&details)`."
    )
    .unwrap();
    writeln!(out, "    #[non_exhaustive]").unwrap();
    writeln!(out, "    #[derive(Debug, Clone, Copy, PartialEq)]").unwrap();
    writeln!(out, "    pub enum EventDetails {{").unwrap();
    for ((ev, fs), om) in evs.iter().zip(&fields).zip(&omitted) {
        writeln!(
            out,
            "        /// `{}`. Present in: {}.",
            ev.code,
            present_in(specs, ev.defs.iter().map(|d| d.0))
        )
        .unwrap();
        out.push_str(&omitted_doc("        ", om));
        if fs.is_empty() {
            writeln!(out, "        {},", ev.name).unwrap();
            continue;
        }
        writeln!(out, "        {} {{", ev.name).unwrap();
        for f in fs {
            if f.optional {
                writeln!(
                    out,
                    "            /// `None` in versions without this field. Present in: {}.",
                    present_in(specs, f.occ.iter().map(|o| o.0))
                )
                .unwrap();
                writeln!(out, "            {}: Option<{}>,", f.name, f.ty.ret()).unwrap();
            } else {
                writeln!(out, "            {}: {},", f.name, f.ty.ret()).unwrap();
            }
        }
        writeln!(out, "        }},").unwrap();
    }
    writeln!(out, "        /// An event code unknown to the decoding version; payload zero-padded to {max_payload} bytes.").unwrap();
    writeln!(
        out,
        "        Unknown {{ code: [u8; 4], payload: [u8; {max_payload}] }},"
    )
    .unwrap();
    writeln!(out, "    }}\n").unwrap();

    for (si, payload) in &payloads {
        let m = &specs[*si].version.module;
        writeln!(
            out,
            "    impl From<&super::{m}::EventDetails> for EventDetails {{"
        )
        .unwrap();
        writeln!(
            out,
            "        fn from(details: &super::{m}::EventDetails) -> Self {{"
        )
        .unwrap();
        writeln!(out, "            use super::{m}::EventDetails as V;").unwrap();
        writeln!(out, "            match *details {{").unwrap();
        for (ev, fs) in evs.iter().zip(&fields) {
            let Some((_, def)) = ev.defs.iter().find(|d| d.0 == *si) else {
                continue;
            };
            let bound: Vec<&str> = fs
                .iter()
                .filter(|f| f.occ.iter().any(|o| o.0 == *si))
                .map(|f| f.name.as_str())
                .collect();
            let pattern = if def.fields.is_empty() {
                format!("V::{}", ev.name)
            } else if bound.is_empty() {
                format!("V::{} {{ .. }}", ev.name)
            } else {
                format!("V::{} {{ {}, .. }}", ev.name, bound.join(", "))
            };
            let value = if fs.is_empty() {
                format!("Self::{}", ev.name)
            } else {
                let inits: Vec<String> = fs
                    .iter()
                    .map(|f| match f.occ.iter().find(|o| o.0 == *si) {
                        Some((_, ft, scale)) => {
                            let e = f.ty.convert(&f.name, ft, *scale);
                            if f.optional {
                                format!("{}: Some({e})", f.name)
                            } else if e == f.name {
                                e
                            } else {
                                format!("{}: {e}", f.name)
                            }
                        }
                        None => format!("{}: None", f.name),
                    })
                    .collect();
                format!("Self::{} {{ {} }}", ev.name, inits.join(", "))
            };
            writeln!(out, "                {pattern} => {value},").unwrap();
        }
        // Field shorthand when the payload needs no zero-padding.
        let payload_init = if *payload == max_payload {
            "payload".to_string()
        } else {
            format!(
                "payload: {{ let mut p = [0u8; {max_payload}]; p[..{payload}].copy_from_slice(&payload); p }}"
            )
        };
        writeln!(
            out,
            "                V::Unknown {{ code, payload }} => Self::Unknown {{ code, {payload_init} }},"
        )
        .unwrap();
        writeln!(out, "            }}").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "    }}\n").unwrap();
    }
}

/// Emit the `common` module: one trait per struct and packet across all
/// versions, `common::EventDetails`, and the `PacketVisitor` dispatch.
fn emit_common(out: &mut String, specs: &[ResolvedSpec]) {
    writeln!(out, "/// Version-independent view of every packet.").unwrap();
    writeln!(out, "///").unwrap();
    writeln!(
        out,
        "/// Each struct and packet has a trait of the same name, implemented by every"
    )
    .unwrap();
    writeln!(
        out,
        "/// version's type. Accessors use a type that fits all versions (narrower integers"
    )
    .unwrap();
    writeln!(
        out,
        "/// widened, quantised values scaled to floats, differing array lengths as slices);"
    )
    .unwrap();
    writeln!(
        out,
        "/// fields missing in some version return `Option`. Fields whose types cannot be"
    )
    .unwrap();
    writeln!(
        out,
        "/// reconciled are only available on the version-specific types."
    )
    .unwrap();
    writeln!(out, "pub mod common {{").unwrap();
    writeln!(out, "    #[allow(unused_imports)]").unwrap();
    writeln!(out, "    use crate::fixed_str::FixedStr;").unwrap();
    writeln!(out, "    use crate::header::PacketHeader;").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "    /// Associated type for struct fields that a version does not have; never constructed.").unwrap();
    writeln!(out, "    #[derive(Debug, Clone, Copy, PartialEq, Eq)]").unwrap();
    writeln!(out, "    pub enum Absent {{}}\n").unwrap();

    // Structs, in first-seen order.
    let mut struct_defs: Vec<(String, VersionFields)> = Vec::new();
    for (si, s) in specs.iter().enumerate() {
        for st in &s.structs {
            match struct_defs.iter_mut().find(|(n, _)| *n == st.name) {
                Some((_, defs)) => defs.push((si, &st.fields[..])),
                None => struct_defs.push((st.name.clone(), vec![(si, &st.fields[..])])),
            }
        }
    }
    for (name, defs) in &struct_defs {
        emit_common_trait(out, specs, name, name, defs, false);
    }

    // Packets, by id.
    let mut packets: BTreeMap<u8, (&str, bool, VersionFields)> = BTreeMap::new();
    for (si, s) in specs.iter().enumerate() {
        for p in &s.packets {
            packets
                .entry(p.id)
                .or_insert_with(|| (p.name.as_str(), p.event_packet, Vec::new()))
                .2
                .push((si, &p.fields[..]));
        }
    }
    emit_common_events(out, specs);
    for (name, event, defs) in packets.values() {
        let concrete = format!("Packet{name}");
        if !*event {
            emit_common_trait(out, specs, name, &concrete, defs, true);
            continue;
        }
        writeln!(
            out,
            "    /// `{concrete}` across versions. Implemented in: {}.",
            present_in(specs, defs.iter().map(|d| d.0))
        )
        .unwrap();
        writeln!(out, "    pub trait {concrete} {{").unwrap();
        writeln!(out, "        /// The packet header.").unwrap();
        writeln!(out, "        fn header(&self) -> &PacketHeader;").unwrap();
        writeln!(out, "        /// The raw 4-byte event code.").unwrap();
        writeln!(out, "        fn event_code(&self) -> [u8; 4];").unwrap();
        writeln!(
            out,
            "        /// The event payload, converted to the cross-version enum."
        )
        .unwrap();
        writeln!(out, "        fn event_details(&self) -> EventDetails;").unwrap();
        writeln!(out, "    }}\n").unwrap();
        for (si, _) in defs {
            let m = &specs[*si].version.module;
            writeln!(out, "    impl {concrete} for super::{m}::{concrete} {{").unwrap();
            writeln!(
                out,
                "        fn header(&self) -> &PacketHeader {{ &self.header }}"
            )
            .unwrap();
            writeln!(
                out,
                "        fn event_code(&self) -> [u8; 4] {{ self.event_code }}"
            )
            .unwrap();
            writeln!(out, "        fn event_details(&self) -> EventDetails {{ EventDetails::from(&self.event_details) }}").unwrap();
            writeln!(out, "    }}\n").unwrap();
        }
    }

    // Visitor
    writeln!(
        out,
        "    /// Per-packet callbacks. Every method defaults to doing nothing, so implement only"
    )
    .unwrap();
    writeln!(
        out,
        "    /// what you need; new versions and packet kinds never break an implementation."
    )
    .unwrap();
    writeln!(out, "    /// Dispatch with `Packet::visit`.").unwrap();
    writeln!(out, "    pub trait PacketVisitor {{").unwrap();
    for (name, _, defs) in packets.values() {
        writeln!(
            out,
            "        /// Present in: {}.",
            present_in(specs, defs.iter().map(|d| d.0))
        )
        .unwrap();
        writeln!(
            out,
            "        fn {}<P: Packet{name}>(&mut self, packet: &P) {{ let _ = packet; }}",
            snake_case(name)
        )
        .unwrap();
    }
    writeln!(out, "    }}\n").unwrap();

    for s in specs {
        let m = &s.version.module;
        writeln!(out, "    impl super::{m}::Packet {{").unwrap();
        writeln!(
            out,
            "        /// Call the matching [`PacketVisitor`] method with this packet."
        )
        .unwrap();
        writeln!(
            out,
            "        pub fn visit<V: PacketVisitor>(&self, visitor: &mut V) {{"
        )
        .unwrap();
        writeln!(out, "            match self {{").unwrap();
        for p in &s.packets {
            writeln!(
                out,
                "                Self::{}(p) => visitor.{}(p),",
                p.name,
                snake_case(&p.name)
            )
            .unwrap();
        }
        writeln!(out, "            }}").unwrap();
        writeln!(out, "        }}").unwrap();
        writeln!(out, "    }}\n").unwrap();
    }
    writeln!(out, "}}\n").unwrap();
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

    // Resolve in sorted order so build errors are reproducible across runs.
    let mut stems: Vec<&String> = raw.keys().collect();
    stems.sort();
    let mut specs: Vec<ResolvedSpec> = stems
        .into_iter()
        .map(|stem| {
            let mut spec = resolve(stem, &raw, &mut Vec::new());
            apply_consts(&mut spec);
            spec
        })
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
    emit_common(&mut all_modules, &specs);
    emit_crate_items(&mut all_modules, &specs);

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let dest = Path::new(&out_dir).join("generated.rs");
    fs::write(&dest, &all_modules)
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", dest.display()));
}
