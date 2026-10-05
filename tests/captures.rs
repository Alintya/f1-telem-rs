//! Replays real game captures from `tests/captures/*.f1cap` (record them with
//! `f1-listen --record FILE`) and fails on anything that suggests the specs disagree with
//! what the game sends. Passes trivially when no captures are present. Also tests the
//! capture format itself, which is private to the tooling and shared with `f1-listen`.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use f1_telem::common::{EventDetails, PacketEvent, PacketVisitor};
use f1_telem::{parse_header, parse_packet, ParseError};

#[allow(dead_code)] // only the parts these tests use
#[path = "../src/bin/listen/capture.rs"]
mod capture;

use capture::CaptureWriter;

fn capture_of(datagrams: &[&[u8]]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut w = CaptureWriter::new(&mut bytes).unwrap();
    for d in datagrams {
        w.write(d).unwrap();
    }
    bytes
}

#[test]
fn capture_round_trip_preserves_order_and_bytes() {
    let big = vec![7u8; 1470];
    let bytes = capture_of(&[b"abc", b"", &big]);
    let read: Vec<&[u8]> = capture::datagrams(&bytes)
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(read, [&b"abc"[..], &b""[..], &big[..]]);
}

#[test]
fn capture_truncated_tail_yields_complete_records_then_one_error() {
    let mut bytes = capture_of(&[b"first", b"second"]);
    bytes.truncate(bytes.len() - 1);
    let mut it = capture::datagrams(&bytes).unwrap();
    assert_eq!(it.next().unwrap().unwrap(), b"first");
    assert_eq!(
        it.next().unwrap().unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    assert!(it.next().is_none());

    // A lone length byte is also truncated.
    let mut bytes = capture_of(&[]);
    bytes.push(3);
    let mut it = capture::datagrams(&bytes).unwrap();
    assert!(it.next().unwrap().is_err());
    assert!(it.next().is_none());
}

#[test]
fn capture_rejects_bad_signature_and_oversized_datagrams() {
    let err = capture::datagrams(b"F1TCAP02").err().unwrap();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);

    let mut bytes = Vec::new();
    let mut w = CaptureWriter::new(&mut bytes).unwrap();
    let err = w.write(&vec![0; 65536]).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(
        bytes,
        capture::MAGIC,
        "nothing written for a rejected datagram"
    );
}

/// Records the code of an event the decoding version does not know.
#[derive(Default)]
struct UnknownEvent(Option<[u8; 4]>);

impl PacketVisitor for UnknownEvent {
    fn event<P: PacketEvent>(&mut self, packet: &P) {
        if let EventDetails::Unknown { code, .. } = packet.event_details() {
            self.0 = Some(code);
        }
    }
}

/// What is wrong with one datagram, if anything.
fn problem(datagram: &[u8]) -> Option<String> {
    let packet = match parse_packet(datagram) {
        Ok(packet) => packet,
        Err(e) => return Some(e.to_string()),
    };
    // Parsing accepts trailing bytes, so the datagram matches the declared size exactly
    // only if dropping its last byte makes the size check fail.
    let shorter = &datagram[..datagram.len() - 1];
    if !matches!(parse_packet(shorter), Err(ParseError::SizeMismatch { .. })) {
        return Some(format!(
            "{} bytes received, spec declares fewer",
            datagram.len()
        ));
    }
    let mut unknown = UnknownEvent::default();
    packet.visit(&mut unknown);
    unknown
        .0
        .map(|code| format!("unknown event code {:?}", String::from_utf8_lossy(&code)))
}

#[test]
fn captured_datagrams_match_specs() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/captures");
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };
    let mut paths: Vec<_> = entries
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "f1cap"))
        .collect();
    paths.sort();

    // (format, packet id, problem) -> (occurrences, first location); real captures
    // repeat each problem thousands of times.
    let mut problems: BTreeMap<(u16, u8, String), (usize, String)> = BTreeMap::new();
    for path in &paths {
        let name = path.file_name().unwrap().to_string_lossy();
        let bytes = fs::read(path).unwrap();
        let datagrams = capture::datagrams(&bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
        for (i, datagram) in datagrams.enumerate() {
            let Ok(datagram) = datagram else {
                break; // recording interrupted mid-write; complete records were checked
            };
            if let Some(p) = problem(datagram) {
                let key = parse_header(datagram).map_or((0, 0), |h| (h.packet_format, h.packet_id));
                problems
                    .entry((key.0, key.1, p))
                    .or_insert((0, format!("{name}#{i}")))
                    .0 += 1;
            }
        }
    }

    let report: Vec<String> = problems
        .iter()
        .map(|((format, id, p), (n, first))| {
            format!("format {format} packet {id}: {p} ({n}x, first at {first})")
        })
        .collect();
    assert!(report.is_empty(), "\n{}", report.join("\n"));
}
