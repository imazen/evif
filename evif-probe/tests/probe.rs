//! End to end on a synthetic reference: sample a family, score it, write an
//! EVIF file, read it back, and check that the file's own digests and scores
//! agree with the pixels it carries.

use evif::{Bpp, Evif};
use evif_probe::family;
use evif_probe::image::Rgb8Image;
use evif_probe::judge;
use evif_probe::registry::Registry;
use evif_probe::rng::Rng;
use evif_probe::search::{self, Baseline};
use evif_probe::write;

/// A 160x128 reference with gradients, an edge and some texture, so every
/// metric has something to measure and no corruption is an identity.
fn reference() -> Rgb8Image {
    let (w, h) = (160u32, 128u32);
    let mut data = Vec::with_capacity((w * h * 3) as usize);
    let mut rng = Rng::new(7);
    for y in 0..h {
        for x in 0..w {
            let edge = if x > w / 2 { 60 } else { 0 };
            let noise = (rng.unit() * 24.0) as u8;
            data.push(((x * 255) / w) as u8 ^ noise);
            data.push((((y * 255) / h) as u8).wrapping_add(edge));
            data.push((((x + y) * 3) % 256) as u8);
        }
    }
    Rgb8Image::new(w, h, data).unwrap()
}

fn judges() -> Vec<Box<dyn judge::Judge>> {
    Registry::builtin().all_judges()
}

fn check_file(bytes: &[u8], reference: &Rgb8Image, expect_bits: bool) -> Evif {
    let f = Evif::from_bytes(bytes).expect("file parses");
    let px = Rgb8Image::from_png(&f.rendition.data).expect("PNGF decodes");
    assert_eq!(px.digest(), f.digests.distorted, "PIXL digest matches DIGS");
    assert_eq!(f.digests.reference, Some(reference.digest()));
    assert_eq!(f.bits.is_some(), expect_bits);
    let scores = f.scores.as_ref().expect("scores present");
    assert_eq!(scores.scores.len(), 4, "zensim, ssim2, butteraugli x2");
    // Re-scoring the rendition reproduces the recorded values exactly: the
    // file reports on itself and the report is checkable.
    let again = judge::score_all(&judges(), reference, &px).unwrap();
    for (a, b) in scores.scores.iter().zip(&again) {
        assert_eq!(a.key(), b.key());
        assert_eq!(a.value, b.value, "{} reproduces", a.key());
    }
    f
}

#[test]
fn zenjpeg_expert_round_trip() {
    let reference = reference();
    let fam = Registry::builtin().family("zenjpeg-expert").unwrap();
    let judges = judges();
    let baseline = Baseline::build(fam.as_ref(), &judges, &reference, &[20.0, 50.0, 80.0])
        .unwrap()
        .expect("codec family has a baseline");
    assert_eq!(baseline.points().len(), 3);
    let bpps: Vec<f64> = baseline.points().iter().map(|p| p.0).collect();
    assert!(
        bpps.windows(2).all(|w| w[0] < w[1]),
        "bpp rises with quality: {bpps:?}"
    );

    let mut rng = Rng::for_key(1, "test");
    let setup = search::Setup {
        family: fam.as_ref(),
        judges: &judges,
        baseline: Some(&baseline),
        reference: &reference,
        target: "ssim2",
    };
    let results = setup.run(&mut rng, 3, |_, _, _| {}, |_, _| {}).unwrap();
    for (sample, enc) in &results {
        assert!(sample.disagreement.is_some());
        assert_eq!(sample.gain.len(), 4);
        let file = write::build(
            fam.as_ref(),
            &reference,
            "synthetic",
            Some("ssim2"),
            sample,
            enc,
        )
        .unwrap();
        let bytes = file.to_bytes().unwrap();
        let back = check_file(&bytes, &reference, true);
        let bits = back.bits.as_ref().unwrap();
        assert_eq!(&bits.codec, b"jpeg");
        assert_eq!(&bits.data[..2], &[0xFF, 0xD8]);
        // Interior bpp counts the JPEG alone.
        let Some(Bpp::Interior(bpp)) = back.bits_per_pixel() else {
            panic!("interior bpp")
        };
        assert_eq!(
            bpp,
            bits.data.len() as f64 * 8.0 / reference.pixels() as f64
        );
        // The recipe reproduces the bitstream byte for byte.
        let recipe = back.recipe.as_ref().unwrap();
        assert_eq!(recipe.family, "zenjpeg-expert");
        let again = fam.encode(&recipe.params, &reference).unwrap();
        assert_eq!(again.bits.unwrap().data, bits.data);
    }
}

#[test]
fn corruption_round_trip() {
    let reference = reference();
    let fam = Registry::builtin().family("corruption").unwrap();
    let judges = judges();
    assert!(
        Baseline::build(fam.as_ref(), &judges, &reference, &[50.0])
            .unwrap()
            .is_none(),
        "synthetic family has no baseline"
    );
    let mut rng = Rng::for_key(3, "test");
    let params = fam.sample(&mut rng, &reference);
    let enc = fam.encode(&params, &reference).unwrap();
    assert_ne!(enc.pixels, reference, "corruption changed pixels");
    let setup = search::Setup {
        family: fam.as_ref(),
        judges: &judges,
        baseline: None,
        reference: &reference,
        target: "ssim2",
    };
    let sample = setup.evaluate(params, &enc).unwrap();
    assert!(sample.gain.is_empty() && sample.disagreement.is_none());
    let file = write::build(fam.as_ref(), &reference, "synthetic", None, &sample, &enc).unwrap();
    let bytes = file.to_bytes().unwrap();
    let back = check_file(&bytes, &reference, false);
    assert_eq!(back.bits_per_pixel(), None, "no bitstream, no claim");
    assert!(family::corruption::slug(&back.recipe.unwrap().params).is_some());
}

#[test]
fn seeded_runs_repeat() {
    let reference = reference();
    let fam = Registry::builtin().family("zenjpeg-expert").unwrap();
    let a = fam.sample(&mut Rng::for_key(9, "img"), &reference);
    let b = fam.sample(&mut Rng::for_key(9, "img"), &reference);
    let c = fam.sample(&mut Rng::for_key(9, "other"), &reference);
    assert_eq!(a, b);
    assert_ne!(a, c);
}

/// Every codec family: a baseline sweep rises in bpp, samples encode and
/// write files whose recipes reproduce their bitstreams.
#[test]
fn every_codec_family_round_trips() {
    let reference = reference();
    let reg = Registry::builtin();
    let judges = judges();
    for name in ["zenjpeg-color", "zenpng-palette"] {
        let fam = reg.family(name).unwrap();
        let baseline = Baseline::build(fam.as_ref(), &judges, &reference, &[10.0, 50.0, 90.0])
            .unwrap()
            .unwrap_or_else(|| panic!("{name} has a baseline"));
        let bpps: Vec<f64> = baseline.points().iter().map(|p| p.0).collect();
        assert!(
            bpps.windows(2).all(|w| w[0] < w[1]),
            "{name} bpp rises: {bpps:?}"
        );
        let setup = search::Setup {
            family: fam.as_ref(),
            judges: &judges,
            baseline: Some(&baseline),
            reference: &reference,
            target: "zensim",
        };
        let mut skipped = 0;
        let results = setup
            .run(
                &mut Rng::for_key(5, name),
                4,
                |_, _, _| {},
                |_, _| skipped += 1,
            )
            .unwrap();
        assert_eq!(results.len(), 4);
        for (sample, enc) in &results {
            let file = write::build(
                fam.as_ref(),
                &reference,
                "synthetic",
                Some("zensim"),
                sample,
                enc,
            )
            .unwrap();
            let back = check_file(&file.to_bytes().unwrap(), &reference, true);
            let bits = back.bits.unwrap();
            let again = fam
                .encode(&back.recipe.unwrap().params, &reference)
                .unwrap();
            assert_eq!(
                again.bits.unwrap().data,
                bits.data,
                "{name} recipe reproduces bytes"
            );
        }
        eprintln!("{name}: {skipped} skipped");
    }
}
