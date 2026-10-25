use evif::{
    Better, Bitstream, Bpp, Error, Evif, Header, Layout, Recipe, Sample, Score, Scores, Source,
    pixel_digest_u8, pixel_digest_u16,
};

fn sample_file() -> Evif {
    let header = Header::new(4, 2, Layout::Rgb, Sample::U8);
    let mut e = Evif::new(
        header,
        b"\x89PNG not really".to_vec(),
        0x1234_5678_9abc_def0,
    );
    e.bits = Some(Bitstream {
        codec: *b"jpeg",
        data: vec![0xFF, 0xD8, 0xFF, 0xD9],
    });
    e.digests.reference = Some(42);
    let mut src = Source::default();
    src.id = Some("cid22/001".into());
    e.source = Some(src);
    e.recipe = Some(Recipe::new(
        "zenjpeg-quant",
        serde_json::json!({"ac_scale": 9.0}),
    ));
    e.scores = Some(Scores::new(vec![
        Score::new("ssim2", 71.5, Better::Higher),
        Score::new("butteraugli", 1.9, Better::Lower).variant("3-norm"),
    ]));
    e
}

#[test]
fn round_trip() {
    let e = sample_file();
    let bytes = e.to_bytes().unwrap();
    assert_eq!(&bytes[..8], &evif::SIGNATURE);
    let back = Evif::from_bytes(&bytes).unwrap();
    assert_eq!(back, e);
    assert_eq!(
        back.scores
            .unwrap()
            .get("butteraugli/3-norm")
            .unwrap()
            .value,
        1.9
    );
    // Interior bpp counts only the bitstream: 4 bytes over 8 pixels.
    assert_eq!(e.bits_per_pixel(), Some(Bpp::Interior(4.0)));
}

#[test]
fn claimed_bpp_only_without_bits() {
    let mut e = sample_file();
    e.claimed_bytes = Some(2);
    assert_eq!(e.bits_per_pixel(), Some(Bpp::Interior(4.0)));
    e.bits = None;
    assert_eq!(e.bits_per_pixel(), Some(Bpp::Claimed(2.0)));
    let back = Evif::from_bytes(&e.to_bytes().unwrap()).unwrap();
    assert_eq!(back.claimed_bytes, Some(2));
}

#[test]
fn every_truncation_is_refused() {
    let bytes = sample_file().to_bytes().unwrap();
    for n in 0..bytes.len() {
        assert!(
            Evif::from_bytes(&bytes[..n]).is_err(),
            "prefix {n} accepted"
        );
    }
}

#[test]
fn every_single_bit_flip_is_refused() {
    let bytes = sample_file().to_bytes().unwrap();
    for i in 0..bytes.len() {
        for bit in 0..8 {
            let mut b = bytes.clone();
            b[i] ^= 1 << bit;
            assert!(
                Evif::from_bytes(&b).is_err(),
                "flip byte {i} bit {bit} accepted"
            );
        }
    }
}

/// Insert a chunk just before SEAL and reseal.
fn with_extra_chunk(tag: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let bytes = sample_file().to_bytes().unwrap();
    let mut out = bytes[..bytes.len() - 16].to_vec();
    out.extend_from_slice(tag);
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body);
    let seal = xxhash_rust::xxh3::xxh3_64(&out);
    out.extend_from_slice(b"SEAL");
    out.extend_from_slice(&8u32.to_le_bytes());
    out.extend_from_slice(&seal.to_le_bytes());
    out
}

#[test]
fn unknown_ancillary_is_kept_and_rewritten() {
    let bytes = with_extra_chunk(b"boos", b"spooky");
    let e = Evif::from_bytes(&bytes).unwrap();
    assert_eq!(e.unknown.len(), 1);
    assert_eq!(&e.unknown[0].tag, b"boos");
    assert_eq!(e.to_bytes().unwrap(), bytes);
}

#[test]
fn unknown_critical_is_refused() {
    let bytes = with_extra_chunk(b"BOOS", b"spooky");
    assert_eq!(Evif::from_bytes(&bytes), Err(Error::Unsupported(*b"BOOS")));
}

#[test]
fn duplicate_known_chunk_is_refused() {
    let bytes = with_extra_chunk(b"clam", &7u64.to_le_bytes());
    let mut e = sample_file();
    e.claimed_bytes = Some(1);
    // sample_file has no clam, so one extra is fine...
    assert!(Evif::from_bytes(&bytes).is_ok());
    // ...but a second one is not.
    let base = e.to_bytes().unwrap();
    let mut out = base[..base.len() - 16].to_vec();
    out.extend_from_slice(b"clam");
    out.extend_from_slice(&8u32.to_le_bytes());
    out.extend_from_slice(&7u64.to_le_bytes());
    let seal = xxhash_rust::xxh3::xxh3_64(&out);
    out.extend_from_slice(b"SEAL");
    out.extend_from_slice(&8u32.to_le_bytes());
    out.extend_from_slice(&seal.to_le_bytes());
    assert_eq!(
        Evif::from_bytes(&out),
        Err(Error::Corrupt("duplicate chunk"))
    );
}

#[test]
fn seal_matches_independent_xxh3() {
    let bytes = sample_file().to_bytes().unwrap();
    let n = bytes.len() - 16;
    let seal = u64::from_le_bytes(bytes[n + 8..].try_into().unwrap());
    assert_eq!(seal, xxhash_rust::xxh3::xxh3_64(&bytes[..n]));
}

fn descriptor(w: u32, h: u32, layout: u8, sample: u8) -> Vec<u8> {
    let mut d = b"EVIF/pdig/v1\0\0\0\0".to_vec();
    d.extend_from_slice(&w.to_le_bytes());
    d.extend_from_slice(&h.to_le_bytes());
    d.extend_from_slice(&[layout, sample]);
    assert_eq!(d.len(), 26);
    d
}

#[test]
fn u8_digest_matches_spec_and_ignores_stride_padding() {
    let (w, h) = (37u32, 5u32);
    let row = w as usize * 4;
    let packed: Vec<u8> = (0..row * h as usize).map(|i| (i * 7 + 3) as u8).collect();
    let mut expect = descriptor(w, h, 4, 1);
    expect.extend_from_slice(&packed);
    let want = xxhash_rust::xxh3::xxh3_64(&expect);
    assert_eq!(
        pixel_digest_u8(w, h, Layout::Rgba, &packed, row).unwrap(),
        want
    );

    let stride = row + 13;
    let mut strided = vec![0xEEu8; stride * h as usize];
    for y in 0..h as usize {
        strided[y * stride..y * stride + row].copy_from_slice(&packed[y * row..(y + 1) * row]);
    }
    assert_eq!(
        pixel_digest_u8(w, h, Layout::Rgba, &strided, stride).unwrap(),
        want
    );
}

#[test]
fn u16_digest_is_little_endian_and_crosses_buffer_boundaries() {
    // 300 RGB pixels per row = 900 samples, more than the 256-sample buffer.
    let (w, h) = (300u32, 3u32);
    let row = w as usize * 3;
    let px: Vec<u16> = (0..row * h as usize)
        .map(|i| (i as u64).wrapping_mul(2654435761) as u16)
        .collect();
    let mut expect = descriptor(w, h, 3, 2);
    for s in &px {
        expect.extend_from_slice(&s.to_le_bytes());
    }
    assert_eq!(
        pixel_digest_u16(w, h, Layout::Rgb, &px, row).unwrap(),
        xxhash_rust::xxh3::xxh3_64(&expect)
    );
}

#[test]
fn digest_separates_geometry() {
    let px = vec![9u8; 24];
    let a = pixel_digest_u8(4, 2, Layout::Rgb, &px, 12).unwrap();
    let b = pixel_digest_u8(2, 4, Layout::Rgb, &px, 6).unwrap();
    let c = pixel_digest_u8(8, 1, Layout::Rgb, &px, 24).unwrap();
    assert!(a != b && b != c && a != c);
}

fn reseal(mut prefix: Vec<u8>) -> Vec<u8> {
    let seal = xxhash_rust::xxh3::xxh3_64(&prefix);
    prefix.extend_from_slice(b"SEAL");
    prefix.extend_from_slice(&8u32.to_le_bytes());
    prefix.extend_from_slice(&seal.to_le_bytes());
    prefix
}

#[test]
fn reference_decode_choices_and_legacy_pixl() {
    use evif::{Rendition, RenditionFormat};
    let png = sample_file();
    let bytes = png.to_bytes().unwrap();
    assert_eq!(bytes[9], 1);
    let mut legacy = bytes[..12].to_vec();
    legacy[9] = 0;
    let mut at = 12;
    while &bytes[at..at + 4] != b"SEAL" {
        let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        if &bytes[at..at + 4] == b"PNGF" {
            legacy.extend_from_slice(b"PIXL");
            legacy.extend_from_slice(&((len + 4) as u32).to_le_bytes());
            legacy.extend_from_slice(b"png ");
            legacy.extend_from_slice(&bytes[at + 8..at + 8 + len]);
        } else {
            legacy.extend_from_slice(&bytes[at..at + 8 + len]);
        }
        at += 8 + len;
    }
    assert_eq!(Evif::from_bytes(&reseal(legacy)).unwrap(), png);
    let mut imij = png.clone();
    imij.rendition = Rendition {
        format: RenditionFormat::Imij,
        data: b"opaque IMI payload".to_vec(),
    };
    assert_eq!(Evif::from_bytes(&imij.to_bytes().unwrap()).unwrap(), imij);
    // Two alternate reference decodes are ambiguous even when their tags differ.
    for (tag, body) in [
        (b"IMIJ", b"imi".as_slice()),
        (b"PNGF", b"png"),
        (b"PIXL", b"png old"),
    ] {
        assert_eq!(
            Evif::from_bytes(&with_extra_chunk(tag, body)),
            Err(Error::Corrupt("duplicate chunk"))
        );
    }
}

#[test]
fn omitted_candidate_keeps_provenance_without_claiming_embedded_bytes() {
    use evif::BitstreamInfo;
    let mut file = sample_file();
    let info = BitstreamInfo::from_bitstream(file.bits.as_ref().unwrap());
    let rendition = file.rendition.clone();
    let recipe = file.recipe.clone();
    let scores = file.scores.clone();
    file.omit_bits();
    assert!(file.bits.is_none());
    assert_eq!(file.bitstream_info, Some(info));
    assert_eq!(file.bits_per_pixel(), Some(Bpp::Recorded(4.0)));
    file.claimed_bytes = Some(999);
    assert_eq!(file.bits_per_pixel(), Some(Bpp::Recorded(4.0)));
    let back = Evif::from_bytes(&file.to_bytes().unwrap()).unwrap();
    assert_eq!(back, file);
    assert_eq!(back.rendition, rendition);
    assert_eq!(back.recipe, recipe);
    assert_eq!(back.scores, scores);
    file.omit_bits();
    assert_eq!(file.bitstream_info, Some(info));
}

#[test]
fn candidate_provenance_mismatch_is_refused_even_after_resealing() {
    use evif::BitstreamInfo;
    let mut file = sample_file();
    let info = BitstreamInfo::from_bitstream(file.bits.as_ref().unwrap());
    file.bitstream_info = Some(info);
    assert_eq!(Evif::from_bytes(&file.to_bytes().unwrap()).unwrap(), file);
    for offset in [0, 4, 12] {
        let mut payload = Vec::from(info.codec);
        payload.extend_from_slice(&info.bytes.to_le_bytes());
        payload.extend_from_slice(&info.xxh3.to_le_bytes());
        payload[offset] ^= 1;
        assert_eq!(
            Evif::from_bytes(&with_extra_chunk(b"BINF", &payload)),
            Err(Error::Corrupt("BINF differs from BITS"))
        );
    }
    file.bits.as_mut().unwrap().data.push(0);
    assert_eq!(
        file.to_bytes(),
        Err(Error::Corrupt("BINF differs from BITS"))
    );
}

#[test]
fn evif_specific_candidate_stays_in_bits() {
    let mut file = sample_file();
    file.bits.as_mut().unwrap().codec = *b"evif";
    let back = Evif::from_bytes(&file.to_bytes().unwrap()).unwrap();
    assert_eq!(back.bits.unwrap().codec, *b"evif");
    assert_eq!(back.rendition.format, evif::RenditionFormat::Png);
}
