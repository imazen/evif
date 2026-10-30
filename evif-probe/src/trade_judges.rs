//! Judges for the trade ladders: the CVVDP reference vote and the luma/chroma diagnostics.
//! Definitions are registered in `docs/trade-ladders.md`; diagnostics never vote.
use crate::{Res, adversarial_judges::sha, image::Rgb8Image, judge::Judge};
use evif::{Better, Score};
use std::{cell::Cell, cell::RefCell, collections::HashMap, rc::Rc};

pub(crate) const CVVDP_IMPLEMENTATION: &str = "cvvdp-cpu-imazen-v0.1.0@zenmetrics-0a61830db89a768ab9462f166d1412e948e4efa9;display=standard_4k";

/// Let the runner keep a handle on a judge it also hands to the scoring loop.
pub(crate) struct Shared<T>(pub(crate) Rc<T>);
impl<T: Judge> Judge for Shared<T> {
    fn name(&self) -> &'static str {
        self.0.name()
    }
    fn keys(&self) -> Vec<String> {
        self.0.keys()
    }
    fn score(&self, r: &Rgb8Image, d: &Rgb8Image) -> Res<Vec<Score>> {
        self.0.score(r, d)
    }
}

/// CVVDP (CPU port) under the `standard_4k` display model, JOD, higher is better.
/// The native per-pixel map is written (uncompressed PFM) only while the gate is set.
pub(crate) struct CvvdpJudge {
    scorers: RefCell<HashMap<(u32, u32), cvvdp::Cvvdp>>,
    pub(crate) maps: std::path::PathBuf,
    pub(crate) gate: Rc<Cell<bool>>,
}
impl CvvdpJudge {
    pub(crate) fn new(maps: std::path::PathBuf, gate: Rc<Cell<bool>>) -> Self {
        Self {
            scorers: RefCell::new(HashMap::new()),
            maps,
            gate,
        }
    }
}
impl Judge for CvvdpJudge {
    fn name(&self) -> &'static str {
        "cvvdp"
    }
    fn keys(&self) -> Vec<String> {
        vec!["cvvdp".into()]
    }
    fn score(&self, r: &Rgb8Image, d: &Rgb8Image) -> Res<Vec<Score>> {
        use std::io::Write;
        let mut scorers = self.scorers.borrow_mut();
        let key = (r.width, r.height);
        if let std::collections::hash_map::Entry::Vacant(slot) = scorers.entry(key) {
            let display: cvvdp::display::CvvdpDisplay = "standard_4k"
                .parse()
                .map_err(|e| format!("cvvdp display: {e}"))?;
            let params = cvvdp::CvvdpParams {
                display: display.model(),
                ..cvvdp::CvvdpParams::default()
            };
            slot.insert(
                cvvdp::Cvvdp::with_geometry(r.width, r.height, params, display.geometry())
                    .map_err(|e| format!("cvvdp: {e}"))?,
            );
        }
        let c = scorers.get_mut(&key).expect("inserted");
        let mut score;
        let mut diffmap = None;
        if self.gate.get() {
            let mut map = Vec::new();
            let jod = c
                .score_with_diffmap(&r.data, &d.data, &mut map)
                .map_err(|e| format!("cvvdp score: {e}"))?;
            score = Score::new("cvvdp", f64::from(jod), Better::Higher);
            let mut material = Vec::with_capacity(r.data.len() * 2 + 128);
            material.extend(r.width.to_le_bytes());
            material.extend(r.height.to_le_bytes());
            material.extend(&r.data);
            material.extend(&d.data);
            material.extend(CVVDP_IMPLEMENTATION.as_bytes());
            let key = sha(&material);
            let name = format!("cvvdp-{key}.pfm");
            let mut bytes = format!("Pf\n{} {}\n-1.0\n", r.width, r.height).into_bytes();
            for row in map.chunks_exact(r.width as usize).rev() {
                for value in row {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
            let path = self.maps.join(&name);
            match std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&path)
            {
                Ok(mut f) => f.write_all(&bytes).map_err(|e| e.to_string())?,
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    if std::fs::read(&path).map_err(|e| e.to_string())? != bytes {
                        return Err("map CAS collision".into());
                    }
                }
                Err(e) => return Err(e.to_string()),
            }
            diffmap = Some(
                serde_json::json!({"path":format!("maps/{name}"),"sha256":sha(&bytes),
                "reference_pixels_sha256":sha(&r.data),"distorted_pixels_sha256":sha(&d.data),
                "format":"PFM grayscale f32 little endian bottom-to-top","width":r.width,"height":r.height}),
            );
        } else {
            let jod = c
                .score(&r.data, &d.data)
                .map_err(|e| format!("cvvdp score: {e}"))?;
            score = Score::new("cvvdp", f64::from(jod), Better::Higher);
        }
        if let Some(m) = diffmap {
            score.extra.insert("diffmap".into(), m);
        }
        Ok(vec![
            score.version("0.1.0").implementation(CVVDP_IMPLEMENTATION),
        ])
    }
}

const PEAK2: f64 = 255.0 * 255.0;
fn psnr(mse: f64) -> f64 {
    if mse <= 0.0 {
        100.0
    } else {
        (10.0 * (PEAK2 / mse).log10()).min(100.0)
    }
}

/// BT.601 luma Y' of the 8-bit sRGB code values (full range, unrounded).
pub(crate) fn luma601(img: &Rgb8Image) -> Vec<f32> {
    img.triples()
        .iter()
        .map(|p| 0.299 * f32::from(p[0]) + 0.587 * f32::from(p[1]) + 0.114 * f32::from(p[2]))
        .collect()
}
/// Full-range BT.601 Cb and Cr of the 8-bit sRGB code values.
fn chroma601(img: &Rgb8Image) -> (Vec<f32>, Vec<f32>) {
    img.triples()
        .iter()
        .map(|p| {
            let (r, g, b) = (f32::from(p[0]), f32::from(p[1]), f32::from(p[2]));
            (
                128.0 - 0.168_736 * r - 0.331_264 * g + 0.5 * b,
                128.0 + 0.5 * r - 0.418_688 * g - 0.081_312 * b,
            )
        })
        .unzip()
}
fn eotf(code: u8) -> f32 {
    let v = f32::from(code) / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn oetf_code(linear: f32) -> u8 {
    let l = linear.clamp(0.0, 1.0);
    let v = if l <= 0.003_130_8 {
        12.92 * l
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    };
    (v * 255.0).round().clamp(0.0, 255.0) as u8
}
/// CIE relative luminance (BT.709 weights, linear light) as 8-bit sRGB code values.
fn luminance_codes(img: &Rgb8Image) -> Vec<u8> {
    let lut: Vec<f32> = (0..=255u8).map(eotf).collect();
    img.triples()
        .iter()
        .map(|p| {
            oetf_code(
                0.2126 * lut[usize::from(p[0])]
                    + 0.7152 * lut[usize::from(p[1])]
                    + 0.0722 * lut[usize::from(p[2])],
            )
        })
        .collect()
}
fn mse_f32(a: &[f32], b: &[f32]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| f64::from(x - y).powi(2))
        .sum::<f64>()
        / a.len() as f64
}
fn grey_image(codes: &[u8], like: &Rgb8Image) -> Res<Rgb8Image> {
    Rgb8Image::new(
        like.width,
        like.height,
        codes.iter().flat_map(|&c| [c, c, c]).collect(),
    )
}

/// Luma and chroma diagnostics. They record how far luma gained and chroma fell; they never vote.
pub(crate) struct LumaDiag;
impl Judge for LumaDiag {
    fn name(&self) -> &'static str {
        "luma-diagnostics"
    }
    fn keys(&self) -> Vec<String> {
        [
            "luma601-psnr",
            "luma601-ssim2",
            "lumrel-psnr",
            "chroma601-psnr",
        ]
        .map(String::from)
        .into()
    }
    fn score(&self, r: &Rgb8Image, d: &Rgb8Image) -> Res<Vec<Score>> {
        let (yr, yd) = (luma601(r), luma601(d));
        let round = |y: &[f32]| -> Vec<u8> {
            y.iter()
                .map(|v| v.round().clamp(0.0, 255.0) as u8)
                .collect()
        };
        let (gr, gd) = (grey_image(&round(&yr), r)?, grey_image(&round(&yd), d)?);
        let ssim2 = crate::adversarial_judges::Frozen.score(&gr, &gd)?[0].value;
        let (lr, ld) = (luminance_codes(r), luminance_codes(d));
        let lum_mse = lr
            .iter()
            .zip(&ld)
            .map(|(a, b)| (f64::from(*a) - f64::from(*b)).powi(2))
            .sum::<f64>()
            / lr.len() as f64;
        let ((cbr, crr), (cbd, crd)) = (chroma601(r), chroma601(d));
        let chroma_mse = (mse_f32(&cbr, &cbd) + mse_f32(&crr, &crd)) / 2.0;
        let note = "diagnostic: never a reference vote";
        let mk = |name: &str, value: f64| {
            Score::new(name, value, Better::Higher)
                .version("trade-v1")
                .implementation(format!("evif-probe trade diagnostics; {note}"))
        };
        Ok(vec![
            mk("luma601-psnr", psnr(mse_f32(&yr, &yd))),
            mk("luma601-ssim2", ssim2),
            mk("lumrel-psnr", psnr(lum_mse)),
            mk("chroma601-psnr", psnr(chroma_mse)),
        ])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: u32, h: u32) -> Rgb8Image {
        let mut data = Vec::new();
        for y in 0..h {
            for x in 0..w {
                data.extend([
                    (x * 255 / w) as u8,
                    (y * 255 / h) as u8,
                    ((x + y) * 255 / (w + h)) as u8,
                ]);
            }
        }
        Rgb8Image::new(w, h, data).unwrap()
    }

    #[test]
    fn identical_images_are_capped_and_chroma_only_edits_spare_luma() {
        let a = gradient(64, 64);
        let scores = LumaDiag.score(&a, &a).unwrap();
        assert!(
            scores
                .iter()
                .filter(|s| s.metric != "luma601-ssim2")
                .all(|s| s.value == 100.0)
        );
        // Move Cb/Cr, keep Y' (BT.601): the luma diagnostic stays high while chroma collapses.
        let mut b = a.clone();
        for p in b.data.as_chunks_mut::<3>().0 {
            let y = 0.299 * f32::from(p[0]) + 0.587 * f32::from(p[1]) + 0.114 * f32::from(p[2]);
            let (cb, cr) = (
                (-0.168_736 * f32::from(p[0]) - 0.331_264 * f32::from(p[1])
                    + 0.5 * f32::from(p[2]))
                    * 0.25,
                (0.5 * f32::from(p[0]) - 0.418_688 * f32::from(p[1]) - 0.081_312 * f32::from(p[2]))
                    * 0.25,
            );
            p[0] = (y + 1.402 * cr).round().clamp(0.0, 255.0) as u8;
            p[1] = (y - 0.344_136 * cb - 0.714_136 * cr)
                .round()
                .clamp(0.0, 255.0) as u8;
            p[2] = (y + 1.772 * cb).round().clamp(0.0, 255.0) as u8;
        }
        let s = LumaDiag.score(&a, &b).unwrap();
        let by = |k: &str| s.iter().find(|x| x.metric == k).unwrap().value;
        assert!(by("luma601-psnr") > 40.0, "luma {}", by("luma601-psnr"));
        assert!(
            by("chroma601-psnr") < 30.0,
            "chroma {}",
            by("chroma601-psnr")
        );
        assert!(by("luma601-psnr") > by("chroma601-psnr") + 10.0);
    }

    #[test]
    fn luminance_codes_are_monotone_in_grey() {
        let grey = |v: u8| Rgb8Image::new(8, 8, vec![v; 8 * 8 * 3]).unwrap();
        let codes: Vec<u8> = [0u8, 64, 128, 255]
            .iter()
            .map(|&v| luminance_codes(&grey(v))[0])
            .collect();
        assert_eq!(codes, vec![0, 64, 128, 255]);
    }
}
