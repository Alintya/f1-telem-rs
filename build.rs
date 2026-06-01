use serde::Deserialize;
use std::collections::HashMap;
use std::fmt::Write as FmtWrite;
use std::fs;
use std::path::Path;

// ---------------------------------------------------------------------------
// Schema data model – deserialized from spec/*.yaml
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct Spec {
    version: VersionInfo,
    structs: Vec<StructDef>,
    packets: Vec<PacketDef>,
    events: Vec<EventDef>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct VersionInfo {
    format: u16,
    year: u8,
    module: String, // e.g. "v2023"
}

#[derive(Debug, Deserialize)]
struct StructDef {
    name: String,
    fields: Vec<FieldDef>,
}

#[derive(Debug, Deserialize)]
struct FieldDef {
    name: String,
    ty: String,
}

#[derive(Debug, Deserialize)]
struct PacketDef {
    name: String,
    id: u8,
    size: usize,
    fields: Vec<FieldDef>,
}

#[derive(Debug, Deserialize)]
struct EventDef {
    name: String,
    code: String,
    #[serde(default)]
    fields: Vec<FieldDef>,
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
    fn parse(s: &str) -> Self {
        let s = s.trim();
        if let Some(inner) = s.strip_prefix("string(").and_then(|r| r.strip_suffix(')')) {
            return Self::String(inner.parse().expect("invalid string length"));
        }
        if let Some(inner) = s.strip_prefix('[').and_then(|r| r.strip_suffix(']')) {
            let (ty_str, count_str) = inner.rsplit_once(';').expect("array must be [T; N]");
            let count: usize = count_str.trim().parse().expect("invalid array count");
            let inner_ty = Self::parse(ty_str.trim());
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
            Self::String(_) => "String".into(),
        }
    }

    /// Byte size of this type. Panics for struct references (must be resolved externally).
    fn byte_size(&self, struct_sizes: &HashMap<String, usize>) -> usize {
        match self {
            Self::U8 | Self::I8 => 1,
            Self::U16 | Self::I16 => 2,
            Self::U32 | Self::I32 | Self::F32 => 4,
            Self::U64 | Self::F64 => 8,
            Self::PrimArray(inner, n) => inner.byte_size(struct_sizes) * n,
            Self::StructRef(name) => *struct_sizes
                .get(name)
                .unwrap_or_else(|| panic!("unknown struct: {name}")),
            Self::StructArray(name, n) => {
                struct_sizes
                    .get(name)
                    .unwrap_or_else(|| panic!("unknown struct: {name}"))
                    * n
            }
            Self::String(n) => *n,
        }
    }

    /// Generate the reader expression for this type.
    fn reader_expr(&self, field_name: &str) -> String {
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
                Self::I8 => format!("reader.read_i8_array::<{n}>()?"),
                Self::U16 => format!("reader.read_u16_array::<{n}>()?"),
                Self::F32 => format!("reader.read_f32_array::<{n}>()?"),
                other => {
                    // For other primitive arrays, generate an inline loop
                    let read = other.reader_expr(field_name);
                    format!(
                        "{{ let mut arr = [Default::default(); {n}]; for v in &mut arr {{ *v = {read}; }} arr }}"
                    )
                }
            },
            Self::StructRef(name) => format!("{name}::decode(reader)?"),
            Self::StructArray(name, n) => format!(
                "{{ let mut arr: [{name}; {n}] = std::array::from_fn(|_| Default::default()); for v in &mut arr {{ *v = {name}::decode(reader)?; }} arr }}"
            ),
            Self::String(n) => format!("reader.read_string({n}, \"{field_name}\")?"),
        }
    }
}

// ---------------------------------------------------------------------------
// Size computation & validation
// ---------------------------------------------------------------------------

fn compute_struct_sizes(spec: &Spec) -> HashMap<String, usize> {
    let mut sizes: HashMap<String, usize> = HashMap::new();

    // Simple iterative resolution (structs may reference other structs defined earlier)
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
                let ft = FieldType::parse(&f.ty);
                match ft.byte_size_opt(&sizes) {
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

    // Verify all structs resolved
    for s in &spec.structs {
        if !sizes.contains_key(&s.name) {
            panic!(
                "could not resolve size for struct '{}' — likely circular or missing dependency",
                s.name
            );
        }
    }

    sizes
}

impl FieldType {
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
}

fn validate_packet_sizes(spec: &Spec, struct_sizes: &HashMap<String, usize>) {
    let header_size: usize = 29;
    for p in &spec.packets {
        let mut body_size = 0usize;
        for f in &p.fields {
            let ft = FieldType::parse(&f.ty);
            body_size += ft.byte_size(struct_sizes);
        }
        let total = header_size + body_size;
        if total != p.size {
            panic!(
                "[{}] packet '{}' (id={}): computed size {} (header {} + body {}) != declared size {}",
                spec.version.module, p.name, p.id, total, header_size, body_size, p.size
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Code generation
// ---------------------------------------------------------------------------

fn generate_module(spec: &Spec) -> String {
    let struct_sizes = compute_struct_sizes(spec);
    validate_packet_sizes(spec, &struct_sizes);

    let mut out = String::with_capacity(64 * 1024);

    let module = &spec.version.module;
    writeln!(
        out,
        "/// Generated from `spec/{}.yaml`. Do not edit.",
        module
    )
    .unwrap();
    writeln!(
        out,
        "#[allow(unused_imports, clippy::needless_update, clippy::large_enum_variant)]"
    )
    .unwrap();
    writeln!(out, "pub mod {module} {{").unwrap();
    writeln!(out, "    use crate::reader::Reader;").unwrap();
    writeln!(out, "    use crate::error::ParseError;").unwrap();
    writeln!(out, "    use crate::header::PacketHeader;").unwrap();
    writeln!(out, "    use crate::version::PacketKind;").unwrap();
    writeln!(out).unwrap();

    // Structs
    for s in &spec.structs {
        emit_struct(&mut out, s);
    }

    // Events
    emit_events(&mut out, &spec.events);

    // Packets
    for p in &spec.packets {
        emit_packet_struct(&mut out, p, &spec.events);
    }

    // Packet enum + dispatch
    emit_packet_enum(&mut out, &spec.packets, spec.version.format);

    writeln!(out, "}}").unwrap();
    out
}

fn emit_struct(out: &mut String, s: &StructDef) {
    writeln!(out, "    #[derive(Debug, Clone, Default)]").unwrap();
    writeln!(out, "    pub struct {} {{", s.name).unwrap();
    for f in &s.fields {
        let ft = FieldType::parse(&f.ty);
        writeln!(out, "        pub {}: {},", f.name, ft.rust_type()).unwrap();
    }
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    // decode impl
    writeln!(out, "    impl {} {{", s.name).unwrap();
    writeln!(
        out,
        "        pub fn decode(reader: &mut Reader<'_>) -> Result<Self, ParseError> {{"
    )
    .unwrap();
    writeln!(out, "            Ok(Self {{").unwrap();
    for f in &s.fields {
        let ft = FieldType::parse(&f.ty);
        writeln!(
            out,
            "                {}: {},",
            f.name,
            ft.reader_expr(&f.name)
        )
        .unwrap();
    }
    writeln!(out, "            }})").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
}

fn emit_events(out: &mut String, events: &[EventDef]) {
    // EventDetails enum
    writeln!(out, "    #[derive(Debug, Clone)]").unwrap();
    writeln!(out, "    pub enum EventDetails {{").unwrap();
    for ev in events {
        if ev.fields.is_empty() {
            writeln!(out, "        {},", ev.name).unwrap();
        } else {
            writeln!(out, "        {} {{", ev.name).unwrap();
            for f in &ev.fields {
                let ft = FieldType::parse(&f.ty);
                writeln!(out, "            {}: {},", f.name, ft.rust_type()).unwrap();
            }
            writeln!(out, "        }},").unwrap();
        }
    }
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    // decode
    writeln!(out, "    impl EventDetails {{").unwrap();
    writeln!(out, "        pub fn decode(code: &[u8; 4], reader: &mut Reader<'_>) -> Result<Self, ParseError> {{").unwrap();
    writeln!(out, "            match code {{").unwrap();
    for ev in events {
        let code = &ev.code;
        assert_eq!(code.len(), 4, "event code must be 4 chars: {code}");
        if ev.fields.is_empty() {
            writeln!(out, "                b\"{code}\" => Ok(Self::{}),", ev.name).unwrap();
        } else {
            writeln!(
                out,
                "                b\"{code}\" => Ok(Self::{} {{",
                ev.name
            )
            .unwrap();
            for f in &ev.fields {
                let ft = FieldType::parse(&f.ty);
                writeln!(
                    out,
                    "                    {}: {},",
                    f.name,
                    ft.reader_expr(&f.name)
                )
                .unwrap();
            }
            writeln!(out, "                }}),").unwrap();
        }
    }
    writeln!(
        out,
        "                other => Err(ParseError::UnknownEventCode(*other)),"
    )
    .unwrap();
    writeln!(out, "            }}").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
}

fn emit_packet_struct(out: &mut String, p: &PacketDef, _events: &[EventDef]) {
    let struct_name = format!("Packet{}", p.name);
    writeln!(out, "    #[derive(Debug, Clone)]").unwrap();
    writeln!(out, "    pub struct {struct_name} {{").unwrap();
    writeln!(out, "        pub header: PacketHeader,").unwrap();

    // For Event packet, emit event_code and event_details instead of raw fields
    if p.name == "Event" {
        writeln!(out, "        pub event_code: [u8; 4],").unwrap();
        writeln!(out, "        pub event_details: EventDetails,").unwrap();
    } else {
        for f in &p.fields {
            let ft = FieldType::parse(&f.ty);
            writeln!(out, "        pub {}: {},", f.name, ft.rust_type()).unwrap();
        }
    }
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    // decode
    writeln!(out, "    impl {struct_name} {{").unwrap();
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

    if p.name == "Event" {
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
            let ft = FieldType::parse(&f.ty);
            writeln!(
                out,
                "                {}: {},",
                f.name,
                ft.reader_expr(&f.name)
            )
            .unwrap();
        }
        writeln!(out, "            }})").unwrap();
    }
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
}

fn emit_packet_enum(out: &mut String, packets: &[PacketDef], format: u16) {
    // Packet enum
    writeln!(out, "    #[derive(Debug, Clone)]").unwrap();
    writeln!(out, "    pub enum Packet {{").unwrap();
    for p in packets {
        writeln!(out, "        {}(Packet{}),", p.name, p.name).unwrap();
    }
    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();

    writeln!(out, "    impl Packet {{").unwrap();
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
        let struct_name = format!("Packet{}", p.name);
        writeln!(out, "                {} => {{", p.id).unwrap();
        writeln!(out, "                    if total_len < {} {{", p.size).unwrap();
        writeln!(out, "                        return Err(ParseError::SizeMismatch {{ expected: {}, actual: total_len }});", p.size).unwrap();
        writeln!(out, "                    }}").unwrap();
        if p.name == "Event" {
            writeln!(
                out,
                "                    let event_code = reader.read_u8_array::<4>()?;"
            )
            .unwrap();
            writeln!(out, "                    let event_details = EventDetails::decode(&event_code, reader)?;").unwrap();
            writeln!(out, "                    Ok(Self::{}({struct_name} {{ header, event_code, event_details }}))", p.name).unwrap();
        } else {
            writeln!(
                out,
                "                    Ok(Self::{}({struct_name} {{",
                p.name
            )
            .unwrap();
            writeln!(out, "                        header,").unwrap();
            for f in &p.fields {
                let ft = FieldType::parse(&f.ty);
                writeln!(
                    out,
                    "                        {}: {},",
                    f.name,
                    ft.reader_expr(&f.name)
                )
                .unwrap();
            }
            writeln!(out, "                    }}))").unwrap();
        }
        writeln!(out, "                }}").unwrap();
    }
    writeln!(out, "                other => Err(ParseError::UnknownPacketId {{ packet_id: other, format: {} }}),", format).unwrap();
    writeln!(out, "            }}").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out).unwrap();

    // header() accessor
    writeln!(out, "        pub fn header(&self) -> &PacketHeader {{").unwrap();
    writeln!(out, "            match self {{").unwrap();
    for p in packets {
        writeln!(out, "                Self::{}(p) => &p.header,", p.name).unwrap();
    }
    writeln!(out, "            }}").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out).unwrap();

    // kind() accessor
    writeln!(out, "        pub fn kind(&self) -> PacketKind {{").unwrap();
    writeln!(out, "            match self {{").unwrap();
    for p in packets {
        writeln!(
            out,
            "                Self::{}(_) => PacketKind::{},",
            p.name, p.name
        )
        .unwrap();
    }
    writeln!(out, "            }}").unwrap();
    writeln!(out, "        }}").unwrap();

    writeln!(out, "    }}").unwrap();
    writeln!(out).unwrap();
}

// ---------------------------------------------------------------------------
// Main build script
// ---------------------------------------------------------------------------

fn main() {
    let spec_dir = Path::new("spec");

    let spec_files = ["f1_2023.yaml", "f1_2024.yaml", "f1_2025.yaml"];

    let mut all_modules = String::new();

    for file in &spec_files {
        let path = spec_dir.join(file);
        println!("cargo::rerun-if-changed={}", path.display());

        let yaml = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

        let spec: Spec = serde_yaml::from_str(&yaml)
            .unwrap_or_else(|e| panic!("failed to parse {}: {e}", path.display()));

        let module_code = generate_module(&spec);
        all_modules.push_str(&module_code);
        all_modules.push('\n');
    }

    let out_dir = std::env::var("OUT_DIR").unwrap();
    let dest = Path::new(&out_dir).join("generated.rs");
    fs::write(&dest, &all_modules)
        .unwrap_or_else(|e| panic!("failed to write {}: {e}", dest.display()));

    println!("cargo::rerun-if-changed=build.rs");
}
