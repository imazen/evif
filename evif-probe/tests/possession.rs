#![cfg(any(feature = "possession-svt", feature = "possession-jxl"))]
use evif_probe::{image::Rgb8Image, registry::Registry, rng::Rng};
use serde_json::json;

fn image() -> Rgb8Image {
    // Odd dimensions, strong chroma edges, smooth ramps and fine texture.
    let (w, h) = (65u32, 67u32);
    let data = (0..h)
        .flat_map(|y| {
            (0..w).flat_map(move |x| {
                [
                    (x * 3 + y) as u8,
                    (y * 3 + x) as u8,
                    if x < 32 { ((x ^ y) * 7) as u8 } else { 200 },
                ]
            })
        })
        .collect();
    Rgb8Image::new(w, h, data).unwrap()
}
fn exercise(name: &str, tag: [u8; 4], changed: serde_json::Value, invalid: serde_json::Value) {
    let family = Registry::builtin()
        .family(name)
        .expect("feature registers family");
    let source = image();
    let params = family.baseline(50.0).unwrap();
    let first = family.encode(&params, &source).unwrap();
    let replay = family
        .encode(&serde_json::from_str(&params.to_string()).unwrap(), &source)
        .unwrap();
    let bits = first.bits.as_ref().unwrap();
    assert_eq!(bits.codec, tag);
    assert!(!bits.data.is_empty());
    assert_eq!(first.bytes(), Some(bits.data.len() as u64));
    assert_eq!(first.pixels, replay.pixels);
    assert_eq!(bits, replay.bits.as_ref().unwrap());
    assert_ne!(
        first.pixels, source,
        "a lossy candidate must not retain pristine pixels"
    );
    assert_eq!((first.pixels.width, first.pixels.height), (65, 67));
    assert_eq!(
        Rgb8Image::from_png(&first.pixels.to_png().unwrap()).unwrap(),
        first.pixels
    );
    let mut altered = params.clone();
    for (k, v) in changed.as_object().unwrap() {
        altered[k] = v.clone();
    }
    let altered = family.encode(&altered, &source).unwrap();
    assert_ne!(
        first.pixels, altered.pixels,
        "possession control must change decoded samples"
    );
    let mut bad = params.clone();
    for (k, v) in invalid.as_object().unwrap() {
        bad[k] = v.clone();
    }
    assert!(family.encode(&bad, &source).is_err());
    let mut wrong_revision = params.clone();
    wrong_revision["implementation"] = json!("different-codec");
    assert!(family.encode(&wrong_revision, &source).is_err());
    let mut unknown = params.clone();
    unknown["misspelled_knob"] = json!(true);
    assert!(family.encode(&unknown, &source).is_err());
    // Aggressive quality receives the same spacing as the high-quality range.
    for q in (5..=95).step_by(10) {
        family
            .encode(&family.baseline(q as f64).unwrap(), &source)
            .unwrap_or_else(|e| panic!("{name} baseline q{q}: {e}"));
    }
    let mut rng = Rng::new(42);
    for i in 0..24 {
        let p = family.sample(&mut rng, &source);
        let encoded = family
            .encode(&p, &source)
            .unwrap_or_else(|e| panic!("{name} draw {i} {p}: {e}"));
        assert_eq!((encoded.pixels.width, encoded.pixels.height), (65, 67));
    }
    // Optional artifact export is explicit; no test is skipped when unset.
    if let Ok(root) = std::env::var("EVIF_POSSESSION_ARTIFACTS") {
        let root = std::path::Path::new(&root);
        std::fs::create_dir_all(root).unwrap();
        let ext = if tag == *b"avif" { "avif" } else { "jxl" };
        std::fs::write(root.join(format!("{name}.{ext}")), &bits.data).unwrap();
        std::fs::write(
            root.join(format!("{name}.png")),
            first.pixels.to_png().unwrap(),
        )
        .unwrap();
        std::fs::write(root.join(format!("{name}.json")), params.to_string()).unwrap();
    }
}
#[cfg(feature = "possession-svt")]
#[test]
fn svt_possession_replays_decoded_candidate() {
    exercise(
        "zenav1-svt",
        *b"avif",
        json!({"chroma_q":[63,-64]}),
        json!({"chroma_q":[64,0]}),
    );
}
#[cfg(feature = "possession-jxl")]
#[test]
fn jxl_possession_replays_decoded_candidate() {
    exercise(
        "zenjxl",
        *b"jxl ",
        json!({"resampling":4}),
        json!({"resampling":3}),
    );
}
