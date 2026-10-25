//! Real APNG carriers through the major-1 container, without metric inference.
use zencodec::animation::FrameDuration;
use zencodec::encode::{AnimationFrameEncoder, EncodeJob, EncoderConfig};
use zenpixels_convert::{Cicp, PixelDescriptor, PixelFormat, PixelSlice};

#[test]
fn hdr_apng_preserves_samples_color_and_fractional_timing() {
    let (width, height) = (13u32, 9u32);
    let stride = width as usize * 8 + 16;
    let out = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("sequence-carriers-{}", std::process::id()));
    std::fs::create_dir(&out).unwrap();
    for cicp in [Cicp::BT2100_PQ, Cicp::BT2100_HLG] {
        let desc = cicp.to_descriptor(PixelFormat::Rgba16);
        let mut encoder = zenpng::PngEncoderConfig::new()
            .with_cicp(Some(cicp))
            .with_downcast(zenpng::DowncastFlags::none())
            .with_preserve_transparent_rgb(true)
            .job()
            .with_loop_count(Some(2))
            .animation_frame_encoder()
            .unwrap();
        let durations = [(1, 24), (1001, 30000), (7, 100)];
        let mut expected = Vec::new();
        for (frame, (num, den)) in durations.iter().enumerate() {
            let mut bytes = vec![0xA5; stride * height as usize];
            let mut packed = Vec::new();
            for y in 0..height as usize {
                for x in 0..width as usize {
                    let i = (y * width as usize + x) as u16;
                    // Preserve low bits, channel differences and RGB beneath alpha=0.
                    let pixel = [
                        i.wrapping_mul(127).wrapping_add(frame as u16),
                        i ^ 1023,
                        65535 - i,
                        if x == 0 { 0 } else { 32769 },
                    ];
                    for (c, value) in pixel.iter().enumerate() {
                        let b = value.to_ne_bytes();
                        bytes[y * stride + x * 8 + c * 2..y * stride + x * 8 + c * 2 + 2]
                            .copy_from_slice(&b);
                        packed.extend_from_slice(&b);
                    }
                }
            }
            let pixels = PixelSlice::new(&bytes, width, height, stride, desc).unwrap();
            encoder
                .push_frame_timed(pixels, FrameDuration::new(*num, *den).unwrap(), None)
                .unwrap();
            expected.push(packed);
        }
        let encoded = encoder.finish(None).unwrap();
        let stem = format!("transfer-{}", cicp.transfer_characteristics);
        std::fs::write(out.join(format!("{stem}.png")), encoded.data()).unwrap();
        let expected_le: Vec<u8> = expected
            .iter()
            .flat_map(|f| {
                f.as_chunks::<2>()
                    .0
                    .iter()
                    .flat_map(|b| u16::from_ne_bytes([b[0], b[1]]).to_le_bytes())
            })
            .collect();
        std::fs::write(out.join(format!("{stem}.rgba64le")), expected_le).unwrap();
        verify_wire_round_trip(&out, &stem, encoded.data(), cicp, &expected);
        let decoded = zenpng::decode_apng(
            encoded.data(),
            &zenpng::PngDecodeConfig::strict(),
            &enough::Unstoppable,
        )
        .unwrap();
        assert_eq!(decoded.frames.len(), durations.len());
        assert_eq!(decoded.info.bit_depth, 16);
        // RGB samples use an identity matrix, regardless of a source YUV matrix code.
        assert_eq!(
            decoded.info.cicp,
            Some(Cicp::new(
                cicp.color_primaries,
                cicp.transfer_characteristics,
                0,
                true
            ))
        );
        assert_eq!(decoded.num_plays, 2);
        for ((frame, expected), (n, d)) in decoded.frames.iter().zip(expected).zip(durations) {
            assert_eq!(frame.pixels.copy_to_contiguous_bytes(), expected);
            assert_eq!(
                (
                    u64::from(frame.frame_info.delay_num),
                    u64::from(frame.frame_info.delay_den)
                ),
                (n, u64::from(d))
            );
        }
    }
    println!("carrier artifacts: {}", out.display());
}

fn verify_wire_round_trip(
    out: &std::path::Path,
    stem: &str,
    encoded: &[u8],
    cicp: Cicp,
    expected: &[Vec<u8>],
) {
    use evif::wire::*;
    let a = Asset {
        id: 1,
        profile: "resolved-rgb/1".into(),
        kind: Kind::Sequence,
        width: 13,
        height: 9,
        channels: Channels::Rgba,
        samples: Samples::U16,
        primaries: Primaries::Bt2020,
        transfer: if cicp.transfer_characteristics == 16 {
            Transfer::Pq
        } else {
            Transfer::Hlg
        },
        domain: if cicp.transfer_characteristics == 16 {
            Domain::DisplayAbsolute
        } else {
            Domain::SceneRelative
        },
        nits_per_unit: None,
        pixel_aspect: Rational::new(1, 1).unwrap(),
        frame_count: U64(3),
        plays: Some(U64(2)),
    };
    let frames: Vec<_> = expected
        .iter()
        .zip([(1, 24), (1001, 30000), (7, 100)])
        .map(|(bytes, (n, d))| {
            let le: Vec<u8> = bytes
                .as_chunks::<2>()
                .0
                .iter()
                .flat_map(|b| u16::from_ne_bytes(*b).to_le_bytes())
                .collect();
            Frame {
                duration: Some(Rational::new(n, d).unwrap()),
                samples: frame_hash(&a, &le, 13 * 8).unwrap(),
            }
        })
        .collect();
    let mut writer = Writer::new(Vec::new(), Limits::default()).unwrap();
    let mut plans = Vec::new();
    let mut assets = Vec::new();
    for id in [1, 2] {
        let a = Asset { id, ..a.clone() };
        writer.asset(&a).unwrap();
        let s = Segment {
            asset: id,
            first_frame: U64(0),
            carrier: Carrier::Apng,
            frames: frames.clone(),
            storage: Storage::Embedded {
                hash: Some(Hash::of(encoded)),
            },
        };
        writer
            .segment(&s, &mut &encoded[..], encoded.len() as u64)
            .unwrap();
        plans.push(serde_json::json!({"segment":s, "path":format!("{stem}.png")}));
        assets.push(a);
    }
    let roots = writer.finish_media().unwrap();
    let score = ScoreRecord {
        schema: 1,
        id: "fixture-metadata".into(),
        supersedes: None,
        candidate: roots[1].hash.clone(),
        reference: Some(roots[0].hash.clone()),
        adapter: "transport-test/1".into(),
        data: serde_json::json!({"status":"not-scored"}),
    };
    writer.score(&score).unwrap();
    let (bytes, _) = writer.finish().unwrap();
    let index = scan(&bytes, Limits::default()).unwrap();
    assert_eq!(index.roots[0].hash, index.roots[1].hash);
    for s in &index.segments {
        let span = s.payload.unwrap();
        let payload = &bytes[span.offset as usize..(span.offset + span.length) as usize];
        assert_eq!(payload, encoded);
        let decoded = zenpng::decode_apng(
            payload,
            &zenpng::PngDecodeConfig::strict(),
            &enough::Unstoppable,
        )
        .unwrap();
        for (decoded, declared) in decoded.frames.iter().zip(&s.segment.frames) {
            let ne = decoded.pixels.copy_to_contiguous_bytes();
            let le: Vec<u8> = ne
                .as_chunks::<2>()
                .0
                .iter()
                .flat_map(|b| u16::from_ne_bytes(*b).to_le_bytes())
                .collect();
            assert_eq!(frame_hash(&a, &le, 13 * 8).unwrap(), declared.samples);
        }
    }
    std::fs::write(out.join(format!("{stem}.evif")), bytes).unwrap();
    std::fs::write(
        out.join(format!("{stem}.plan.json")),
        serde_json::to_vec_pretty(&serde_json::json!({"assets":assets,"segments":plans})).unwrap(),
    )
    .unwrap();
    std::fs::write(
        out.join(format!("{stem}.score.json")),
        serde_json::to_vec_pretty(&score).unwrap(),
    )
    .unwrap();
}

#[test]
fn apng_refuses_unrepresentable_timing_instead_of_rounding() {
    let pixels = [3, 5, 7, 255].repeat(4);
    let mut encoder = zenpng::PngEncoderConfig::new()
        .job()
        .animation_frame_encoder()
        .unwrap();
    let frame = PixelSlice::new(&pixels, 2, 2, 8, PixelDescriptor::RGBA8_SRGB).unwrap();
    assert!(
        encoder
            .push_frame_timed(frame, FrameDuration::new(1, 65536).unwrap(), None)
            .is_err()
    );
}
