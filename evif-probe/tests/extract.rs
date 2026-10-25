//! Extraction preserves both the original codec bytes and the scored rendition.

use std::fs;

use enough::Unstoppable;
use evif::Evif;
use evif_probe::{cli, image::Rgb8Image, registry::Registry};

#[test]
fn png_bitstream_and_rendition_have_distinct_paths() {
    let reg = Registry::builtin();
    let (width, height) = (96, 64);
    let data: Vec<u8> = (0..width * height)
        .flat_map(|i| [(i % 251) as u8, ((i * 7) % 256) as u8, (i / width) as u8])
        .collect();
    let reference = Rgb8Image::new(width, height, data).unwrap();
    let tmp = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"));
    fs::create_dir_all(&tmp).unwrap();
    let base = tmp.join(format!("extract-test-{}", std::process::id()));
    fs::create_dir(&base).unwrap();

    for name in ["zenpng-palette", "zenjpeg-expert"] {
        let family = reg.family(name).unwrap();
        let mut encoded = family
            .encode(&family.baseline(60.0).unwrap(), &reference)
            .unwrap();
        if name == "zenpng-palette" {
            // An interior codec may carry metadata absent from PIXL. Use the
            // encoder API to guarantee distinct bytes without changing pixels.
            let config = zenpng::EncodeConfig::default().with_srgb_intent(Some(0));
            encoded.bits.as_mut().unwrap().data = zenpng::encode_rgb8(
                encoded.pixels.imgref(),
                None,
                &config,
                &Unstoppable,
                &Unstoppable,
            )
            .unwrap();
        }
        let bits = encoded.bits.as_ref().unwrap();
        if name == "zenpng-palette" {
            assert_eq!(Rgb8Image::from_png(&bits.data).unwrap(), encoded.pixels);
        }
        let rendition = encoded.pixels.to_png().unwrap();
        assert_ne!(bits.data, rendition, "test needs distinct representations");
        let mut file = Evif::new(
            encoded.pixels.header(),
            rendition.clone(),
            encoded.pixels.digest(),
        );
        file.bits = encoded.bits.clone();
        let input = base.join(format!("{name}.evif"));
        fs::write(&input, file.to_bytes().unwrap()).unwrap();
        let out = base.join(name);
        cli::run(
            &reg,
            &[
                "extract".into(),
                input.display().to_string(),
                out.display().to_string(),
            ],
        )
        .unwrap();

        let extension = String::from_utf8_lossy(&bits.codec).trim().to_string();
        let bit_path = if extension == "png" {
            out.join("bitstreams").join(format!("{name}.png"))
        } else {
            out.join(format!("{name}.{extension}"))
        };
        assert_eq!(
            fs::read(bit_path).unwrap(),
            bits.data,
            "original codec bytes"
        );
        assert_eq!(
            fs::read(out.join(format!("{name}.png"))).unwrap(),
            rendition
        );
    }
}
