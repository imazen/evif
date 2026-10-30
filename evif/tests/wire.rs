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

#[test]
fn sample_hashes_match_independent_algorithms_and_exclude_padding() {
    use sha2::{Digest, Sha256};
    // Cover short and long XXH3 input paths, multiple sample types, and padding.
    for width in [1, 2, 7, 32, 257] {
        for samples in [Samples::U8, Samples::U16, Samples::F16, Samples::F32] {
            let mut a = asset(1, Kind::Still);
            a.width = width;
            a.samples = samples;
            let row = width as usize * 4 * samples.bytes();
            let stride = row + 13;
            let mut padded = vec![0xa5; stride * 2];
            let mut canonical = b"EVIF/frame/1\0".to_vec();
            canonical.extend_from_slice(&width.to_le_bytes());
            canonical.extend_from_slice(&2u32.to_le_bytes());
            canonical.extend_from_slice(&[4, samples as u8]);
            for y in 0..2 {
                for x in 0..row {
                    padded[y * stride + x] = (x.wrapping_mul(37) + y * 19) as u8;
                }
                canonical.extend_from_slice(&padded[y * stride..y * stride + row]);
            }
            let both = frame_hash(&a, &padded, stride).unwrap();
            let fast = frame_hash_xxh3(&a, &padded, stride).unwrap();
            assert_eq!(
                both.canonical.digest(),
                format!("{:032x}", xxhash_rust::xxh3::xxh3_128(&canonical))
            );
            assert_eq!(
                both.checks,
                vec![
                    Hash::new(
                        "sha256",
                        &Sha256::digest(&canonical)
                            .iter()
                            .map(|b| format!("{b:02x}"))
                            .collect::<String>()
                    )
                    .unwrap()
                ]
            );
            assert_eq!(fast.canonical, both.canonical);
            assert!(fast.checks.is_empty());
            padded[row] ^= 1; // padding is not a sample
            assert_eq!(frame_hash(&a, &padded, stride).unwrap(), both);
            padded[0] ^= 1;
            let changed = frame_hash(&a, &padded, stride).unwrap();
            assert_ne!(changed.canonical, both.canonical);
            assert_ne!(changed.checks, both.checks);
        }
    }
}

#[test]
fn optional_sample_sha_does_not_change_identity_but_canonical_xxh3_does() {
    let a = asset(1, Kind::Sequence);
    let make = |mode| {
        let mut s = segment(&a, b"x");
        for f in &mut s.frames {
            match mode {
                1 => f.samples.checks.clear(),
                2 => f.samples.checks = vec![Hash::of(b"different check")],
                3 | 4 => {
                    let value = u128::from_str_radix(f.samples.canonical.digest(), 16).unwrap();
                    let value = value ^ if mode == 3 { 1 } else { 1u128 << 127 };
                    f.samples.canonical = Hash::new("xxh3-128", &format!("{value:032x}")).unwrap();
                }
                _ => (),
            }
        }
        let mut writer = Writer::new(Vec::new(), Limits::default()).unwrap();
        writer.asset(&a).unwrap();
        writer.segment(&s, &mut &b"x"[..], 1).unwrap();
        writer.finish_media().unwrap();
        let (bytes, index) = writer.finish().unwrap();
        assert_eq!(scan(&bytes, Limits::default()).unwrap().roots, index.roots);
        index.roots
    };
    // A container cannot verify decoded-sample checks; these are root semantics.
    assert_eq!(make(0), make(1));
    assert_eq!(make(0), make(2));
    assert_ne!(make(0), make(3));
    assert_ne!(make(0), make(4)); // the high half must also bind media identity
}

#[test]
fn sample_hash_json_requires_algorithm_and_valid_digest() {
    let fast = SampleHashes {
        canonical: Hash::new("xxh3-128", "0123456789abcdeffedcba9876543210").unwrap(),
        checks: vec![],
    };
    let encoded = serde_json::to_string(&fast).unwrap();
    assert_eq!(
        encoded,
        r#"{"canonical":{"algorithm":"xxh3-128","digest":"0123456789abcdeffedcba9876543210"}}"#
    );
    assert_eq!(
        serde_json::from_str::<SampleHashes>(&encoded).unwrap(),
        fast
    );
    for invalid in [
        r#"{}"#,
        r#"{"sha256":null}"#,
        r#"{"xxh3-64":"0123456789abcdef"}"#,
        r#"{"xxh3-128":"0123456789abcdeffedcba9876543210"}"#,
        r#"{"canonical":123}"#,
        r#"{"canonical":"0123456789abcdeffedcba9876543210"}"#,
        r#"{"canonical":{"algorithm":"xxh3-128","digest":"0123456789abcdeffedcba9876543210"},"other":1}"#,
        r#"{"canonical":{"algorithm":"xxh3-128","digest":"0123456789abcdeffedcba9876543210"},"checks":[{"algorithm":"xxh3-128","digest":"0123456789abcdeffedcba9876543210"}]}"#,
    ] {
        assert!(
            serde_json::from_str::<SampleHashes>(invalid).is_err(),
            "{invalid}"
        );
    }
    for invalid in [
        r#"{"algorithm":"","digest":"00"}"#,
        r#"{"algorithm":"SHA256","digest":"00"}"#,
        r#"{"algorithm":"1hash","digest":"00"}"#,
        r#"{"algorithm":"hash/name","digest":"00"}"#,
        r#"{"algorithm":"future","digest":""}"#,
        r#"{"algorithm":"future","digest":"0"}"#,
        r#"{"algorithm":"future","digest":"AA"}"#,
        r#"{"algorithm":"future","digest":"zz"}"#,
        r#"{"algorithm":"future","digest":"é"}"#,
        r#"{"algorithm":"xxh3-128","digest":"0123456789abcdef"}"#,
        r#"{"algorithm":"sha256","digest":"0123456789abcdeffedcba9876543210"}"#,
        r#"{"algorithm":"future","digest":"00","other":1}"#,
        r#"{"algorithm":"future"}"#,
        r#"{"digest":"00"}"#,
        r#"{"algorithm":"future","digest":0}"#,
        r#"{"algorithm":"future","algorithm":"future","digest":"00"}"#,
    ] {
        assert!(serde_json::from_str::<Hash>(invalid).is_err(), "{invalid}");
    }
    assert!(Hash::new(&"a".repeat(65), "00").is_err());
}

#[test]
fn sample_hash_full_width_and_zero_padding_are_canonical() {
    for text in [
        "00000000000000000000000000000000",
        "00000000000000000000000000000001",
        "00000000000000010000000000000000",
        "ffffffffffffffffffffffffffffffff",
    ] {
        let hash = Hash::new("xxh3-128", text).unwrap();
        assert_eq!(hash.digest(), text);
        assert_eq!(
            serde_json::from_str::<Hash>(&serde_json::to_string(&hash).unwrap()).unwrap(),
            hash
        );
    }
}

#[test]
fn future_hash_widths_round_trip_and_bind_algorithm_and_every_byte() {
    let a = asset(1, Kind::Still);
    let out = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("future-hashes-{}", std::process::id()));
    // A cached target directory can hold the directory of an earlier process that had the same id.
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir(&out).unwrap();
    let make = |canonical: Hash, checks: Vec<Hash>| {
        let mut s = segment(&a, b"x");
        s.frames[0].samples = SampleHashes { canonical, checks };
        // An external address may also use an algorithm unavailable locally.
        s.storage = Storage::External {
            uri: "https://example.invalid/future.imi".into(),
            hash: Hash::new("future-payload", &"be".repeat(64)).unwrap(),
        };
        let mut writer = Writer::new(Vec::new(), Limits::default()).unwrap();
        writer.asset(&a).unwrap();
        writer.segment(&s, &mut io::empty(), 0).unwrap();
        writer.finish_media().unwrap();
        let (bytes, index) = writer.finish().unwrap();
        for decoded in [
            scan(&bytes, Limits::default()).unwrap(),
            read(&mut Cursor::new(&bytes), Limits::default()).unwrap(),
        ] {
            assert_eq!(decoded.segments[0].segment, s);
            assert_eq!(decoded.roots, index.roots);
        }
        assert!(matches!(
            scan(
                &bytes,
                Limits {
                    max_json_bytes: 100,
                    ..Limits::default()
                }
            ),
            Err(Error::Limit(_))
        ));
        (index.roots[0].hash.clone(), bytes)
    };
    // 256, 512, 1024 and 4096 bits; there is no 128/256-bit storage ceiling.
    for bytes in [32, 64, 128, 512] {
        let text = "a5".repeat(bytes);
        let hash = Hash::new("future-hash", &text).unwrap();
        let (baseline, encoded) = make(hash.clone(), vec![]);
        std::fs::write(out.join(format!("digest-{}.evif", bytes * 8)), encoded).unwrap();
        assert_eq!(
            baseline,
            make(
                hash.clone(),
                vec![Hash::new("future-check", &"3c".repeat(bytes)).unwrap()]
            )
            .0
        );
        assert_ne!(
            baseline,
            make(Hash::new("another-hash", &text).unwrap(), vec![]).0
        );
        assert_ne!(
            baseline,
            make(
                Hash::new("future-hash", &(text.clone() + "00")).unwrap(),
                vec![]
            )
            .0
        );
        let mut changed = text;
        changed.replace_range(bytes * 2 - 2.., "a4");
        assert_ne!(
            baseline,
            make(Hash::new("future-hash", &changed).unwrap(), vec![]).0
        );
    }
}

#[test]
fn unsupported_verification_algorithms_and_duplicate_checks_are_refused() {
    let a = asset(1, Kind::Still);
    let mut s = segment(&a, b"x");
    s.frames[0].samples.checks = vec![Hash::of(b"first"), Hash::of(b"second")];
    let mut writer = Writer::new(Vec::new(), Limits::default()).unwrap();
    writer.asset(&a).unwrap();
    assert!(writer.segment(&s, &mut &b"x"[..], 1).is_err());

    let mut s = segment(&a, b"x");
    s.storage = Storage::Embedded {
        hash: Some(Hash::new("future-payload", &"ab".repeat(64)).unwrap()),
    };
    let mut writer = Writer::new(Vec::new(), Limits::default()).unwrap();
    writer.asset(&a).unwrap();
    assert!(matches!(
        writer.segment(&s, &mut &b"x"[..], 1),
        Err(Error::Unsupported(_))
    ));

    let (bytes, _) = file();
    for target in [b"SEGM", b"MEND"] {
        let mut altered = HEADER.to_vec();
        let mut pos = HEADER.len();
        while pos < bytes.len() {
            let tag: &[u8; 4] = bytes[pos..pos + 4].try_into().unwrap();
            let length = u64::from_le_bytes(bytes[pos + 4..pos + 12].try_into().unwrap()) as usize;
            let body = &bytes[pos + 12..pos + 12 + length];
            if tag == target {
                let mut value: serde_json::Value = serde_json::from_slice(body).unwrap();
                if tag == b"SEGM" {
                    value["storage"]["hash"]["algorithm"] = "future-payload".into();
                } else {
                    value[0]["hash"]["algorithm"] = "future-root".into();
                }
                altered.extend(raw(tag, &serde_json::to_vec(&value).unwrap()));
            } else {
                altered.extend(raw(tag, body));
            }
            pos += 12 + length;
        }
        assert!(matches!(
            scan(&altered, Limits::default()),
            Err(Error::Unsupported(_))
        ));
        assert!(matches!(
            read(&mut Cursor::new(&altered), Limits::default()),
            Err(Error::Unsupported(_))
        ));
    }
}

fn segment(a: &Asset, bytes: &[u8]) -> Segment {
    Segment {
        asset: a.id,
        first_frame: U64(0),
        carrier: Carrier::Apng,
        frames: frames(a),
        storage: Storage::Embedded {
            hash: Some(Hash::of(bytes)),
        },
    }
}
fn score(hash: Hash, id: &str) -> ScoreRecord {
    ScoreRecord {
        schema: 1,
        id: id.into(),
        supersedes: None,
        candidate: hash.clone(),
        reference: Some(hash.clone()),
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
    w.bitstream_info(&BitstreamInfo {
        asset: 1,
        codec: "evif".into(),
        format: "urn:imazen:evif-bits:1".into(),
        version: SyntaxVersion::Declared { value: "1".into() },
        profile: None,
        level: None,
        parameters: serde_json::json!({"prediction":"none"}),
        bytes: U64(7),
        hash: Some(Hash::of(b"encoded")),
    })
    .unwrap();
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
        assert_eq!(index.roots[0].hash, index.roots[1].hash); // local IDs/roles not hashed
        assert_eq!(index.bitstreams[0].codec, *b"evif");
        assert_eq!(
            index.bitstream_info[&1].version,
            SyntaxVersion::Declared { value: "1".into() }
        );
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
fn encoded_format_survives_omitted_bytes_and_embedded_bytes_are_verified() {
    assert!(
        serde_json::from_str::<SyntaxVersion>(r#"{"kind":"unversioned","value":"2"}"#).is_err()
    );
    assert!(serde_json::from_str::<SyntaxVersion>(r#"{"kind":"future"}"#).is_err());
    let mut w = Writer::new(Vec::new(), Limits::default()).unwrap();
    let a = asset(1, Kind::Sequence);
    let payload = b"carrier";
    w.asset(&a).unwrap();
    w.segment(
        &segment(&a, payload),
        &mut &payload[..],
        payload.len() as u64,
    )
    .unwrap();
    let info = BitstreamInfo {
        asset: 1,
        codec: "cool".into(),
        format: "urn:imazen:cool-chic-codestream".into(),
        version: SyntaxVersion::Unversioned,
        profile: Some("low-latency".into()),
        level: None,
        parameters: serde_json::json!({"model":"checkpoint-7","decoder_mode":2}),
        bytes: U64(3),
        hash: Some(Hash::of(b"abc")),
    };
    w.bitstream_info(&info).unwrap();
    w.finish_media().unwrap();
    let (omitted, _) = w.finish().unwrap();
    let index = scan(&omitted, Limits::default()).unwrap();
    assert_eq!(index.bitstream_info[&1], info);
    assert!(index.bitstreams.is_empty());
    assert_eq!(
        read(&mut Cursor::new(&omitted), Limits::default())
            .unwrap()
            .bitstream_info[&1],
        info
    );

    let (mut embedded, index) = file();
    let offset = index.bitstreams[0].payload.offset as usize;
    embedded[offset] ^= 1;
    assert!(matches!(
        scan(&embedded, Limits::default()),
        Err(Error::Invalid("BITS SHA-256 mismatch"))
    ));
    assert!(matches!(
        read(&mut Cursor::new(&embedded), Limits::default()),
        Err(Error::Invalid("BITS SHA-256 mismatch"))
    ));
}

#[test]
fn bits_need_matching_preceding_format_descriptor() {
    let (bytes, _) = file();
    let tag = bytes.windows(4).position(|v| v == b"BINF").unwrap();
    let length = u64::from_le_bytes(bytes[tag + 4..tag + 12].try_into().unwrap()) as usize;
    let mut missing = bytes.clone();
    missing.drain(tag..tag + 12 + length);
    assert!(matches!(
        scan(&missing, Limits::default()),
        Err(Error::Invalid("BITS requires preceding BINF"))
    ));

    let mut wrong_codec = bytes.clone();
    let bits = wrong_codec.windows(4).position(|v| v == b"BITS").unwrap();
    wrong_codec[bits + 12 + 4..bits + 12 + 8].copy_from_slice(b"jpeg");
    assert!(matches!(
        scan(&wrong_codec, Limits::default()),
        Err(Error::Invalid("BITS codec differs from BINF"))
    ));
}

#[test]
fn append_preserves_media_bytes_and_identity_and_detects_partial_tail() {
    let (bytes, index) = file();
    let mut output = Cursor::new(bytes.clone());
    let first = score(index.roots[0].hash.clone(), "first");
    append_score(&mut output, &first, Limits::default()).unwrap();
    let after_first = output.get_ref().len();
    let mut second = score(first.candidate.clone(), "second");
    second.supersedes = Some("first".into());
    second.reference = Some(Hash::new("future-reference", &"bc".repeat(64)).unwrap());
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
    let mut wrong = score(first.candidate.clone(), "wrong-algorithm");
    wrong.candidate = Hash::new("future-root", first.candidate.digest()).unwrap();
    assert!(append_score(&mut output, &wrong, Limits::default()).is_err());
    assert_eq!(output.get_ref(), &before); // matching bytes cannot replace the algorithm
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
        .position(|w| w == index.roots[0].hash.digest().as_bytes())
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
                hash: Hash::of(&[i as u8]),
            },
        };
        w.segment(&s, &mut io::empty(), 0).unwrap();
    }
    let roots = w.finish_media().unwrap();
    assert_eq!(roots[0].hash, baseline.roots[0].hash);
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
        storage: Storage::Embedded { hash: None },
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
    let s = serde_json::to_string(&score(index.roots[0].hash.clone(), "bad")).unwrap();
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

#[test]
fn shared_imi_pages_bind_samples_without_duplicating_the_carrier() {
    let reference = asset(1, Kind::Still);
    let candidate = asset(2, Kind::Still);
    let owner = Segment {
        asset: 1,
        first_frame: U64(0),
        carrier: Carrier::Imi,
        frames: frames(&reference),
        storage: Storage::EmbeddedPages {
            hash: Some(Hash::of(b"self contained page bundle")),
            first_page: U64(0),
            total_pages: U64(2),
        },
    };
    let shared = Segment {
        asset: 2,
        first_frame: U64(0),
        carrier: Carrier::Imi,
        frames: frames(&candidate),
        storage: Storage::SharedPages {
            source_asset: 1,
            source_first_frame: U64(0),
            first_page: U64(1),
        },
    };
    let encode = |owner: &Segment, shared: &Segment| -> Result<Vec<u8>, Error> {
        let mut writer = Writer::new(Vec::new(), Limits::default())?;
        writer.asset(&reference)?;
        writer.asset(&candidate)?;
        writer.segment(owner, &mut &b"self contained page bundle"[..], 26)?;
        writer.segment(shared, &mut std::io::empty(), 0)?;
        writer.finish_media()?;
        writer.finish().map(|(bytes, _)| bytes)
    };
    let bytes = encode(&owner, &shared).unwrap();
    let index = read(&mut Cursor::new(&bytes), Limits::default()).unwrap();
    let sliced = scan(&bytes, Limits::default()).unwrap();
    assert_eq!(sliced.segments, index.segments);
    assert_eq!(sliced.roots, index.roots);
    let mut corrupt = bytes.clone();
    let offset = index.segments[0].payload.unwrap().offset as usize;
    corrupt[offset] ^= 1;
    assert!(scan(&corrupt, Limits::default()).is_err());
    assert!(read(&mut Cursor::new(&corrupt), Limits::default()).is_err());
    assert_eq!(index.segments[0].payload, index.segments[1].payload);
    assert_eq!(index.roots[0].hash, index.roots[1].hash);
    for page in [2, u64::MAX] {
        let mut bad = shared.clone();
        bad.storage = Storage::SharedPages {
            source_asset: 1,
            source_first_frame: U64(0),
            first_page: U64(page),
        };
        assert!(encode(&owner, &bad).is_err());
    }
    let mut bad = shared.clone();
    bad.storage = Storage::SharedPages {
        source_asset: 2,
        source_first_frame: U64(0),
        first_page: U64(0),
    };
    assert!(encode(&owner, &bad).is_err());
    let mut bad_owner = owner.clone();
    bad_owner.carrier = Carrier::Png;
    assert!(encode(&bad_owner, &shared).is_err());
}

#[test]
fn shared_pages_reject_signal_conflicts_chains_and_payload_bytes() {
    let a = asset(1, Kind::Still);
    for conflict in 0..5 {
        let mut b = asset(2, Kind::Still);
        match conflict {
            0 => b.width += 1,
            1 => b.primaries = Primaries::Bt709,
            2 => b.samples = Samples::U8,
            3 => b.pixel_aspect = Rational::new(2, 1).unwrap(),
            _ => b.channels = Channels::Rgb,
        }
        let mut w = Writer::new(Vec::new(), Limits::default()).unwrap();
        w.asset(&a).unwrap();
        w.asset(&b).unwrap();
        let mut owner = segment(&a, b"x");
        owner.carrier = Carrier::Imi;
        owner.storage = Storage::EmbeddedPages {
            hash: None,
            first_page: U64(0),
            total_pages: U64(3),
        };
        w.segment(&owner, &mut &b"x"[..], 1).unwrap();
        let shared = Segment {
            asset: 2,
            storage: Storage::SharedPages {
                source_asset: 1,
                source_first_frame: U64(0),
                first_page: U64(1),
            },
            ..owner
        };
        assert!(w.segment(&shared, &mut io::empty(), 0).is_err());
    }
    for payload in [false, true] {
        let mut w = Writer::new(Vec::new(), Limits::default()).unwrap();
        for id in 1..=3 {
            w.asset(&asset(id, Kind::Still)).unwrap();
        }
        let mut owner = segment(&a, b"x");
        owner.carrier = Carrier::Imi;
        owner.storage = Storage::EmbeddedPages {
            hash: None,
            first_page: U64(0),
            total_pages: U64(3),
        };
        w.segment(&owner, &mut &b"x"[..], 1).unwrap();
        let shared = Segment {
            asset: 2,
            storage: Storage::SharedPages {
                source_asset: 1,
                source_first_frame: U64(0),
                first_page: U64(1),
            },
            ..owner
        };
        if payload {
            assert!(w.segment(&shared, &mut &b"x"[..], 1).is_err());
        } else {
            w.segment(&shared, &mut io::empty(), 0).unwrap();
            let chained = Segment {
                asset: 3,
                storage: Storage::SharedPages {
                    source_asset: 2,
                    source_first_frame: U64(0),
                    first_page: U64(2),
                },
                ..shared
            };
            assert!(w.segment(&chained, &mut io::empty(), 0).is_err());
        }
    }
}
