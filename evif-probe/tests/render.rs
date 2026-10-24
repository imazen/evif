//! `evif render`: baseline qualities and explicit parameter vectors produce
//! decodable renditions whose recorded sizes match a direct encode.

use std::fs;

use evif_probe::cli;
use evif_probe::image::Rgb8Image;
use evif_probe::registry::Registry;

fn reference() -> Rgb8Image {
    let (w, h) = (96u32, 64u32);
    let mut data = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            data.push(((x * 255) / w) as u8);
            data.push(((y * 255) / h) as u8 ^ ((x * 7) as u8 & 31));
            data.push((((x + y) * 5) % 256) as u8);
        }
    }
    Rgb8Image::new(w, h, data).unwrap()
}

fn rows(dir: &std::path::Path) -> Vec<Vec<String>> {
    fs::read_to_string(dir.join("render.tsv"))
        .unwrap()
        .lines()
        .skip(1)
        .map(|l| l.split('\t').map(str::to_string).collect())
        .collect()
}

#[test]
fn baseline_and_params_render_match_direct_encodes() {
    let reg = Registry::builtin();
    let fam = reg.family("zenjpeg-expert").unwrap();
    let base = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("render-test");
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    let r = reference();
    let src = base.join("ref.png");
    fs::write(&src, r.to_png().unwrap()).unwrap();

    // Baseline qualities: one row each, bpp rising with quality.
    let out = base.join("q");
    let args: Vec<String> = ["render", "--family", "zenjpeg-expert", "--out"]
        .iter()
        .map(|s| s.to_string())
        .chain([
            out.display().to_string(),
            "--qualities".into(),
            "20,80".into(),
        ])
        .chain([src.display().to_string()])
        .collect();
    cli::run(&reg, &args).unwrap();
    let q = rows(&out);
    assert_eq!(q.len(), 2);
    let bpp: Vec<f64> = q.iter().map(|r| r[3].parse().unwrap()).collect();
    assert!(bpp[0] < bpp[1], "q20 {} < q80 {}", bpp[0], bpp[1]);
    for (row, quality) in q.iter().zip([20.0, 80.0]) {
        let enc = fam.encode(&fam.baseline(quality).unwrap(), &r).unwrap();
        assert_eq!(row[2], enc.bytes().unwrap().to_string());
        let bitstream = out
            .join("bitstreams")
            .join(format!("ref-zenjpeg-expert-q{quality:05.1}.jpeg"));
        assert_eq!(
            fs::read(bitstream).unwrap(),
            enc.bits.as_ref().unwrap().data
        );
        let png = Rgb8Image::from_png(&fs::read(&row[4]).unwrap()).unwrap();
        assert_eq!(
            png.digest(),
            enc.pixels.digest(),
            "rendition pixels match a direct encode"
        );
    }

    // Explicit parameters: the q80 vector given as JSON renders the same file.
    let params = base.join("p.jsonl");
    fs::write(&params, format!("{}\n", fam.baseline(80.0).unwrap())).unwrap();
    let out_p = base.join("p");
    let args: Vec<String> = ["render", "--family", "zenjpeg-expert", "--out"]
        .iter()
        .map(|s| s.to_string())
        .chain([
            out_p.display().to_string(),
            "--params".into(),
            params.display().to_string(),
        ])
        .chain([src.display().to_string()])
        .collect();
    cli::run(&reg, &args).unwrap();
    let p = rows(&out_p);
    assert_eq!(p.len(), 1);
    assert_eq!(p[0][2], q[1][2], "same parameters, same byte count");
    assert_eq!(fs::read(&p[0][4]).unwrap(), fs::read(&q[1][4]).unwrap());
}

#[test]
fn render_requires_exactly_one_source_of_parameters() {
    let reg = Registry::builtin();
    let args: Vec<String> = ["render", "--family", "zenjpeg-expert", "--out", "unused"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert!(cli::run(&reg, &args).is_err());
}
