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
