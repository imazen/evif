#![cfg(feature = "std")]
use evif::wire::*;
use std::io::{self, Cursor, Read, Write};

fn asset(id: u32, kind: Kind) -> Asset {
    Asset {
        id,
        profile: "resolved-rgb/1".into(),
        kind,
        width: 2,
        height: 2,
        channels: Channels::Rgba,
        samples: Samples::U16,
        primaries: Primaries::Bt2020,
        transfer: Transfer::Pq,
        domain: Domain::DisplayAbsolute,
        nits_per_unit: None,
        pixel_aspect: Rational::new(1, 1).unwrap(),
        frame_count: U64(if kind == Kind::Still { 1 } else { 2 }),
        plays: (kind == Kind::Sequence).then_some(U64(1)),
    }
}
fn frames(a: &Asset) -> Vec<Frame> {
    (0..a.frame_count.0)
        .map(|i| Frame {
            duration: (a.kind == Kind::Sequence).then(|| {
                Rational::new(
                    if i == 0 { 1 } else { 1001 },
                    if i == 0 { 24 } else { 30000 },
                )
                .unwrap()
            }),
            samples: frame_hash(a, &[i as u8; 32], 16).unwrap(),
        })
        .collect()
}
fn segment(a: &Asset, bytes: &[u8]) -> Segment {
    Segment {
        asset: a.id,
        first_frame: U64(0),
        carrier: Carrier::Apng,
        frames: frames(a),
        storage: Storage::Embedded {
            sha256: Some(Hash::of(bytes)),
        },
    }
}
fn score(hash: Hash, id: &str) -> ScoreRecord {
    ScoreRecord {
        schema: 1,
        id: id.into(),
        supersedes: None,
        candidate: hash,
        reference: Some(hash),
        adapter: "test/1".into(),
        data: serde_json::json!({"view":{"ppd":60},"score":9.25}),
    }
}
fn file() -> (Vec<u8>, Index) {
    let mut w = Writer::new(Vec::new(), Limits::default()).unwrap();
    // Opaque fixture payloads intentionally test the container, not a codec.
    let payload = b"opaque carrier test bytes";
    for id in [2, 1] {
        let a = asset(id, Kind::Sequence);
        w.asset(&a).unwrap();
        w.segment(
            &segment(&a, payload),
            &mut &payload[..],
            payload.len() as u64,
        )
        .unwrap();
    }
    w.bitstream(1, *b"evif", &mut &b"encoded"[..], 7).unwrap();
    w.record(*b"note", br#"{"caption":"editable"}"#).unwrap();
    w.finish_media().unwrap();
    w.finish().unwrap()
}
fn raw(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
    [tag.as_slice(), &(body.len() as u64).to_le_bytes(), body].concat()
}

#[test]
fn symmetric_assets_stream_and_slice_have_identical_semantics() {
    let (bytes, expected) = file();
    for index in [
        scan(&bytes, Limits::default()).unwrap(),
        read(&mut Cursor::new(&bytes), Limits::default()).unwrap(),
    ] {
        assert_eq!(index.assets, expected.assets);
        assert_eq!(index.segments, expected.segments);
        assert_eq!(index.roots, expected.roots);
        assert_eq!(index.roots[0].sha256, index.roots[1].sha256); // local IDs/roles not hashed
        assert_eq!(index.bitstreams[0].codec, *b"evif");
        let span = index.bitstreams[0].payload;
        assert_eq!(
            &bytes[span.offset as usize..(span.offset + span.length) as usize],
            b"encoded"
        );
        assert_eq!(index.ancillary[0].tag, *b"note");
    }
    assert!(evif::Evif::from_bytes(&bytes).is_err());
}

#[test]
fn append_preserves_media_bytes_and_identity_and_detects_partial_tail() {
    let (bytes, index) = file();
    let mut output = Cursor::new(bytes.clone());
    let first = score(index.roots[0].sha256, "first");
    append_score(&mut output, &first, Limits::default()).unwrap();
    let after_first = output.get_ref().len();
    let mut second = score(first.candidate, "second");
    second.supersedes = Some("first".into());
    append_score(&mut output, &second, Limits::default()).unwrap();
    assert_eq!(&output.get_ref()[..bytes.len()], bytes);
    let got = scan(output.get_ref(), Limits::default()).unwrap();
    assert_eq!(got.roots, index.roots);
    assert_eq!(got.scores, [first.clone(), second]);
    for end in bytes.len() + 1..output.get_ref().len() {
        if end == after_first {
            continue;
        } // a completed append is a valid file
        assert!(
            scan(&output.get_ref()[..end], Limits::default()).is_err(),
            "cut {end}"
        );
    }
    let before = output.get_ref().clone();
    assert!(append_score(&mut output, &first, Limits::default()).is_err());
    assert_eq!(output.get_ref(), &before); // duplicate refused before writing
}

#[test]
fn every_media_truncation_and_forged_payload_hash_is_rejected() {
    let (bytes, index) = file();
    for end in 0..bytes.len() {
        assert!(scan(&bytes[..end], Limits::default()).is_err(), "cut {end}");
    }
    let mut bad = bytes.clone();
    bad[index.segments[0].payload.unwrap().offset as usize] ^= 1;
    assert!(
        scan(&bad, Limits::default())
            .unwrap_err()
            .to_string()
            .contains("SHA-256")
    );
    let mut bad = bytes;
    let root_offset = bad
        .windows(64)
        .position(|w| w == index.roots[0].sha256.to_string().as_bytes())
        .unwrap();
    bad[root_offset] = if bad[root_offset] == b'0' { b'1' } else { b'0' };
    assert!(
        scan(&bad, Limits::default())
            .unwrap_err()
            .to_string()
            .contains("roots")
    );
}

#[test]
fn notes_do_not_change_media_identity_but_interpretation_does() {
    let (mut bytes, index) = file();
    let off = bytes.windows(8).position(|w| w == b"editable").unwrap();
    bytes[off..off + 8].copy_from_slice(b"reworded");
    assert_eq!(scan(&bytes, Limits::default()).unwrap().roots, index.roots);
    // A coherent but different interpretation cannot retain the old media root.
    let off = bytes.windows(4).position(|w| w == b"\"pq\"").unwrap();
    bytes[off..off + 4].copy_from_slice(b"\"zz\"");
    assert!(scan(&bytes, Limits::default()).is_err());
}

#[test]
fn repacking_segmentation_and_external_storage_do_not_change_identity() {
    let (_, baseline) = file();
    let a = asset(77, Kind::Sequence);
    let mut w = Writer::new(Vec::new(), Limits::default()).unwrap();
    w.asset(&a).unwrap();
    for (i, frame) in frames(&a).into_iter().enumerate() {
        let s = Segment {
            asset: a.id,
            first_frame: U64(i as u64),
            carrier: Carrier::Png,
            frames: vec![frame],
            storage: Storage::External {
                uri: format!("https://example.invalid/{i}.png"),
                sha256: Hash::of(&[i as u8]),
            },
        };
        w.segment(&s, &mut io::empty(), 0).unwrap();
    }
    let roots = w.finish_media().unwrap();
    assert_eq!(roots[0].sha256, baseline.roots[0].sha256);
    let (bytes, _) = w.finish().unwrap();
    assert!(
        scan(&bytes, Limits::default())
            .unwrap()
            .segments
            .iter()
            .all(|s| s.payload.is_none())
    );
}

#[test]
fn float_bits_and_strided_rows_are_exact_and_do_not_require_gain_maps() {
    let mut a = asset(1, Kind::Still);
    a.samples = Samples::F32;
    a.transfer = Transfer::Linear;
    a.nits_per_unit = Some(Rational::new(1, 1).unwrap());
    let bits = [
        0u32, 0x80000000, 0x7fc12345, 0x7f800000, 1, 0xff800000, 0x3f800000, 0x7fa56789,
    ];
    let mut padded = Vec::new();
    let mut packed = Vec::new();
    for _ in 0..2 {
        for b in bits {
            padded.extend_from_slice(&b.to_le_bytes());
            packed.extend_from_slice(&b.to_le_bytes());
        }
        padded.extend_from_slice(&[0xa5; 7]);
    }
    assert_eq!(
        frame_hash(&a, &padded, 39).unwrap(),
        frame_hash(&a, &packed, 32).unwrap()
    );
    let mut changed = packed.clone();
    changed[4] ^= 1;
    assert_ne!(
        frame_hash(&a, &changed, 32).unwrap(),
        frame_hash(&a, &packed, 32).unwrap()
    );
    assert!(frame_hash(&a, &packed, usize::MAX).is_err());
    let mut w = Writer::new(Vec::new(), Limits::default()).unwrap();
    w.asset(&a).unwrap();
    let s = Segment {
        asset: 1,
        first_frame: U64(0),
        carrier: Carrier::Imi,
        frames: vec![Frame {
            duration: None,
            samples: frame_hash(&a, &packed, 32).unwrap(),
        }],
        storage: Storage::Embedded { sha256: None },
    };
    w.segment(&s, &mut &b"opaque IMI fixture"[..], 18).unwrap();
    w.finish_media().unwrap();
    assert_eq!(
        scan(&w.finish().unwrap().0, Limits::default())
            .unwrap()
            .assets[&1],
        a
    );
}

#[test]
fn malformed_lengths_versions_and_extensions_fail_without_allocating_payloads() {
    let (bytes, _) = file();
    let mut future_minor = bytes.clone();
    future_minor[10] = 99;
    scan(&future_minor, Limits::default()).unwrap();
    for (off, value) in [(8, 2), (12, 1)] {
        let mut b = bytes.clone();
        b[off] = value;
        assert!(scan(&b, Limits::default()).is_err());
    }
    let mut b = bytes.clone();
    b.extend(raw(b"MORE", b"unknown critical"));
    assert!(scan(&b, Limits::default()).is_err());
    let mut b = bytes.clone();
    b.extend(raw(b"more", b"unknown ancillary"));
    assert_eq!(scan(&b, Limits::default()).unwrap().ancillary.len(), 2);
    let mut b = bytes;
    b.extend(b"more");
    b.extend(u64::MAX.to_le_bytes());
    assert!(scan(&b, Limits::default()).is_err());
    assert!(read(&mut Cursor::new(&b), Limits::default()).is_err());
}

#[test]
fn coverage_and_order_and_limits_are_enforced() {
    let a = asset(1, Kind::Sequence);
    for mutation in 0..6 {
        let mut w = Writer::new(Vec::new(), Limits::default()).unwrap();
        w.asset(&a).unwrap();
        let mut s = segment(&a, b"x");
        match mutation {
            0 => s.first_frame = U64(1),
            1 => s.asset = 2,
            2 => s.frames[0].duration = None,
            3 => {
                s.frames[0].duration = Some(Rational {
                    num: U64(2),
                    den: U64(48),
                })
            }
            4 => s.carrier = Carrier::Png,
            5 => s.frames[0].duration = Some(Rational::new(1, 65536).unwrap()),
            _ => unreachable!(),
        }
        assert!(w.segment(&s, &mut &b"x"[..], 1).is_err());
        assert!(w.finish().is_err());
    }
    let (b, _) = file();
    for limit in [
        Limits {
            max_assets: 1,
            ..Limits::default()
        },
        Limits {
            max_json_bytes: 16,
            ..Limits::default()
        },
        Limits {
            max_records: 2,
            ..Limits::default()
        },
        Limits {
            max_frames_per_segment: 1,
            ..Limits::default()
        },
        Limits {
            max_total_frames: 1,
            ..Limits::default()
        },
        Limits {
            max_total_json_bytes: 20,
            ..Limits::default()
        },
    ] {
        assert!(matches!(scan(&b, limit), Err(Error::Limit(_))));
    }
}

#[test]
fn u64_json_is_exact_and_noncanonical_forms_are_rejected() {
    let largest = U64(u64::MAX);
    assert_eq!(
        serde_json::to_string(&largest).unwrap(),
        "\"18446744073709551615\""
    );
    assert_eq!(
        serde_json::from_str::<U64>("\"18446744073709551615\"").unwrap(),
        largest
    );
    for bad in [
        "1",
        "\"01\"",
        "\"+1\"",
        "\"-0\"",
        "\"18446744073709551616\"",
    ] {
        assert!(serde_json::from_str::<U64>(bad).is_err());
    }
}

#[test]
fn json_duplicate_keys_unknown_core_fields_and_pending_payloads_are_refused() {
    let a = asset(1, Kind::Sequence);
    let body = serde_json::to_string(&a).unwrap();
    for bad in [
        body.replacen('{', "{\"id\":1,", 1),
        body.replacen('{', "{\"unknown\":1,", 1),
    ] {
        let bytes = [HEADER.as_slice(), &raw(b"ASST", bad.as_bytes())].concat();
        assert!(scan(&bytes, Limits::default()).is_err());
    }
    let (mut bytes, index) = file();
    let s = serde_json::to_string(&score(index.roots[0].sha256, "bad")).unwrap();
    let s = s.replace("\"ppd\":60", "\"ppd\":60,\"ppd\":20");
    bytes.extend(raw(b"scor", s.as_bytes()));
    assert!(
        scan(&bytes, Limits::default())
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );
    let mut w = Writer::new(Vec::new(), Limits::default()).unwrap();
    w.asset(&a).unwrap();
    w.record(*b"SEGM", &serde_json::to_vec(&segment(&a, b"x")).unwrap())
        .unwrap();
    assert!(w.record(*b"note", b"cannot intervene").is_err());
    assert!(w.finish_media().is_err());
}

#[test]
fn failed_source_or_sink_cannot_produce_a_completed_file() {
    let a = asset(1, Kind::Sequence);
    let mut w = Writer::new(Vec::new(), Limits::default()).unwrap();
    w.asset(&a).unwrap();
    assert!(w.segment(&segment(&a, b"abc"), &mut &b"ab"[..], 3).is_err());
    assert!(w.finish_media().is_err());
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("injected"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    assert!(Writer::new(Broken, Limits::default()).is_err());
}

#[test]
fn streaming_offsets_exceed_four_gib_without_a_large_allocation() {
    // Virtual zero stream: no 4 GiB fixture is allocated or written to disk.
    let (bytes, _) = file();
    let len = u64::from(u32::MAX) + 17;
    let header = [b"more".as_slice(), &len.to_le_bytes()].concat();
    let mut source = Cursor::new(&bytes)
        .chain(Cursor::new(header))
        .chain(io::repeat(0).take(len));
    let index = read(&mut source, Limits::default()).unwrap();
    assert_eq!(index.file_end, bytes.len() as u64 + 12 + len);
    assert_eq!(index.ancillary.last().unwrap().payload.length, len);
}
