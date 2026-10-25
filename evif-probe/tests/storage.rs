//! Storage policy changes neither scored pixels nor recorded candidate cost.
use evif::{Bitstream, Bpp, Evif};
use evif_probe::{cli, image::Rgb8Image, registry::Registry};
use std::{fs, path::PathBuf};

fn root(name: &str) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("storage-{name}-{}", std::process::id()));
    fs::create_dir_all(&path).unwrap();
    path
}
fn run(reg: &Registry, args: &[&str]) -> Result<(), String> {
    cli::run(reg, &args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
}
fn image() -> Rgb8Image {
    Rgb8Image::new(64, 64, (0..64 * 64 * 3).map(|i| (i * 7) as u8).collect()).unwrap()
}
#[test]
fn repack_omits_only_candidate_and_native_export_refuses_absence() {
    let reg = Registry::builtin();
    let root = root("repack");
    let image = image();
    let mut file = Evif::new(image.header(), image.to_png().unwrap(), image.digest());
    file.bits = Some(Bitstream {
        codec: *b"evif",
        data: vec![1, 3, 5, 7],
    });
    let input = root.join("input.evif");
    let output = root.join("output.evif");
    fs::write(&input, file.to_bytes().unwrap()).unwrap();
    run(
        &reg,
        &[
            "repack",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            "--omit-bits",
        ],
    )
    .unwrap();
    let back = Evif::from_bytes(&fs::read(&output).unwrap()).unwrap();
    assert_eq!(back.rendition, file.rendition);
    assert_eq!(back.digests, file.digests);
    assert!(back.bits.is_none());
    assert!(matches!(back.bits_per_pixel(), Some(Bpp::Recorded(_))));
    run(&reg, &["verify", output.to_str().unwrap()]).unwrap();
    let out = root.join("native");
    assert!(
        run(
            &reg,
            &[
                "extract",
                output.to_str().unwrap(),
                out.to_str().unwrap(),
                "--format",
                "native"
            ]
        )
        .is_err()
    );
    assert!(!out.exists());
    run(
        &reg,
        &[
            "extract",
            input.to_str().unwrap(),
            out.to_str().unwrap(),
            "--format",
            "native",
        ],
    )
    .unwrap();
    assert_eq!(
        fs::read(out.join("input.evifbits")).unwrap(),
        vec![1, 3, 5, 7]
    );
    assert!(!out.join("input.png").exists());
    let before = fs::read(&output).unwrap();
    assert!(
        run(
            &reg,
            &[
                "repack",
                input.to_str().unwrap(),
                output.to_str().unwrap(),
                "--omit-bits"
            ]
        )
        .is_err()
    );
    assert_eq!(fs::read(output).unwrap(), before);
}
#[test]
fn render_modes_preserve_native_bytes_and_original_cost() {
    let reg = Registry::builtin();
    let root = root("render");
    let image = image();
    let input = root.join("source.png");
    fs::write(&input, image.to_png().unwrap()).unwrap();
    let fam = reg.family("zenjpeg-expert").unwrap();
    let enc = fam.encode(&fam.baseline(60.0).unwrap(), &image).unwrap();
    for format in ["png", "native", "evif"] {
        let out = root.join(format);
        let mut args = vec![
            "render",
            "--family",
            "zenjpeg-expert",
            "--qualities",
            "60",
            "--out",
            out.to_str().unwrap(),
            "--format",
            format,
        ];
        if format != "native" {
            args.push("--omit-bits");
        }
        args.push(input.to_str().unwrap());
        run(&reg, &args).unwrap();
        assert!(!out.join("bitstreams").exists());
        let ledger = fs::read_to_string(out.join("render.tsv")).unwrap();
        let row: Vec<_> = ledger.lines().nth(1).unwrap().split('\t').collect();
        assert_eq!(row[2], enc.bytes().unwrap().to_string());
        assert_eq!(row[8], "jpeg");
        assert_eq!(
            row[9],
            format!(
                "{:016x}",
                zenhash::xxh3_64(&enc.bits.as_ref().unwrap().data)
            )
        );
        let data = fs::read(row[7]).unwrap();
        match format {
            "png" => assert_eq!(Rgb8Image::from_png(&data).unwrap(), enc.pixels),
            "native" => assert_eq!(data, enc.bits.as_ref().unwrap().data),
            "evif" => {
                let file = Evif::from_bytes(&data).unwrap();
                assert!(file.bits.is_none());
                assert_eq!(file.bitstream_info.unwrap().bytes, enc.bytes().unwrap());
                assert_eq!(
                    Rgb8Image::from_png(&file.rendition.data).unwrap(),
                    enc.pixels
                );
                assert_eq!(file.recipe.unwrap().params, fam.baseline(60.0).unwrap());
            }
            _ => unreachable!(),
        }
    }
}
#[test]
fn private_codec_and_contradictory_requests_fail_before_output() {
    let reg = Registry::builtin();
    let root = root("unsupported");
    for extra in [
        vec!["--format", "imij"],
        vec!["--format", "native", "--omit-bits"],
        vec!["--format", "evif", "--rendition", "evif"],
    ] {
        let out = root.join(format!("out-{}", extra.join("-")));
        let mut args = vec![
            "render",
            "--family",
            "zenjpeg-expert",
            "--qualities",
            "60",
            "--out",
            out.to_str().unwrap(),
        ];
        args.extend(extra);
        args.push("missing.png");
        assert!(run(&reg, &args).is_err());
        assert!(!out.exists());
    }
}
