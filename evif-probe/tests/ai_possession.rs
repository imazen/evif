#![cfg(any(feature = "possession-jpegai", feature = "possession-coolchic"))]
use evif_probe::{image::Rgb8Image, registry::Registry};
use serde_json::json;

fn source() -> Rgb8Image {
    Rgb8Image::new(
        8,
        8,
        (0..8 * 8)
            .flat_map(|i| [i as u8 * 3, 255 - i as u8 * 2, i as u8])
            .collect(),
    )
    .unwrap()
}

fn validation(name: &str) {
    let family = Registry::builtin().family(name).unwrap();
    let params = family.baseline(50.0).unwrap();
    let mut wrong_revision = params.clone();
    wrong_revision["implementation"] = json!("wrong");
    assert!(family.encode(&wrong_revision, &source()).is_err());
    let mut unknown = params;
    unknown["misspelled_knob"] = json!(true);
    assert!(family.encode(&unknown, &source()).is_err());
}

#[cfg(feature = "possession-jpegai")]
#[test]
fn jpegai_recipe_validation() {
    validation("zenjpegai");
    let family = Registry::builtin().family("zenjpegai").unwrap();
    let mut bad = family.baseline(50.0).unwrap();
    bad["model_id"] = json!(4);
    assert!(family.encode(&bad, &source()).is_err());
}

#[cfg(feature = "jpegai-models-test")]
#[test]
fn jpegai_native_bytes_decode_to_scored_pixels() {
    let models = std::env::var_os("ZENJPEGAI_MODELS")
        .expect("ZENJPEGAI_MODELS must point to the upstream checkpoint directory");
    assert!(std::path::Path::new(&models).is_dir());
    let source = Rgb8Image::new(
        64,
        64,
        (0..64 * 64)
            .flat_map(|i| {
                let x = (i % 64) as u8;
                let y = (i / 64) as u8;
                [x * 4, y * 4, (x ^ y) * 4]
            })
            .collect(),
    )
    .unwrap();
    let family = Registry::builtin().family("zenjpegai").unwrap();
    let params = family.baseline(50.0).unwrap();
    assert_eq!(params["model_sha256"].as_str().unwrap().len(), 64);
    let result = family.encode(&params, &source).unwrap();
    let mut wrong_model = params;
    wrong_model["model_sha256"] = serde_json::json!("0".repeat(64));
    assert!(family.encode(&wrong_model, &source).is_err());
    let bits = result.bits.unwrap();
    assert_eq!(bits.codec, *b"jaic");
    assert!(bits.data.starts_with(&[0xff, 0x80, 0xff, 0x82]));
    assert_eq!((result.pixels.width, result.pixels.height), (64, 64));
    let decoded = zenjpegai::Decoder::new(&models).decode(&bits.data).unwrap();
    assert_eq!(
        decoded.data,
        result
            .pixels
            .data
            .iter()
            .map(|&v| u16::from(v))
            .collect::<Vec<_>>()
    );
}

#[cfg(feature = "possession-coolchic")]
#[test]
fn coolchic_native_bytes_decode_to_scored_pixels() {
    validation("zencoolchic");
    let family = Registry::builtin().family("zencoolchic").unwrap();
    let params = family.baseline(50.0).unwrap();
    let result = family.encode(&params, &source()).unwrap();
    let bits = result.bits.unwrap();
    assert_eq!(bits.codec, *b"cool");
    assert!(!bits.data.is_empty());
    let decoded = zencoolchic::decode(&bits.data, &zencoolchic::Limits::default()).unwrap();
    assert_eq!(decoded.to_rgb8().unwrap(), result.pixels.data);
}
