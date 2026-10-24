//! Metric judges: full-reference metrics that score a rendition against its
//! source. One judge may report several variants (butteraugli's max norm and
//! 3-norm). The search optimizes for one score key and reports the rest.

use evif::{Better, Score};

use crate::Res;
use crate::image::Rgb8Image;
use crate::pins;

pub trait Judge {
    /// Metric name as written in `scor`.
    fn name(&self) -> &'static str;
    /// Score keys this judge produces (`metric` or `metric/variant`).
    fn keys(&self) -> Vec<String>;
    fn score(&self, reference: &Rgb8Image, distorted: &Rgb8Image) -> Res<Vec<Score>>;
}

/// zensim, current codec-target profile.
pub struct Zensim {
    z: zensim::Zensim,
}

impl Zensim {
    pub fn new() -> Self {
        Self {
            z: zensim::Zensim::new(zensim::ZensimProfile::codec_target()),
        }
    }
}

impl Default for Zensim {
    fn default() -> Self {
        Self::new()
    }
}

impl Judge for Zensim {
    fn name(&self) -> &'static str {
        "zensim"
    }

    fn keys(&self) -> Vec<String> {
        vec!["zensim".into()]
    }

    fn score(&self, reference: &Rgb8Image, distorted: &Rgb8Image) -> Res<Vec<Score>> {
        let (w, h) = (reference.width as usize, reference.height as usize);
        let r = zensim::RgbSlice::new(reference.triples(), w, h);
        let d = zensim::RgbSlice::new(distorted.triples(), w, h);
        let out = self.z.compute(&r, &d).map_err(|e| format!("zensim: {e}"))?;
        Ok(vec![
            Score::new("zensim", out.score(), Better::Higher)
                .version(pins::ZENSIM.version)
                .implementation(pins::ZENSIM.implementation("zensim")),
        ])
    }
}

/// SSIMULACRA2 via fast-ssim2.
pub struct Ssim2;

impl Judge for Ssim2 {
    fn name(&self) -> &'static str {
        "ssim2"
    }

    fn keys(&self) -> Vec<String> {
        vec!["ssim2".into()]
    }

    fn score(&self, reference: &Rgb8Image, distorted: &Rgb8Image) -> Res<Vec<Score>> {
        use fast_ssim2::{PixelDescriptor, PixelSlice, compute_ssimulacra2};
        fn slice(img: &Rgb8Image) -> Res<PixelSlice<'_>> {
            PixelSlice::new(
                &img.data,
                img.width,
                img.height,
                img.stride(),
                PixelDescriptor::RGB8_SRGB,
            )
            .map_err(|e| format!("ssim2 input: {e}"))
        }
        let (r, d) = (slice(reference)?, slice(distorted)?);
        let v = compute_ssimulacra2(&r, &d).map_err(|e| format!("ssim2: {e}"))?;
        Ok(vec![
            Score::new("ssim2", v, Better::Higher)
                .version(pins::FAST_SSIM2.version)
                .implementation(pins::FAST_SSIM2.implementation("fast-ssim2")),
        ])
    }
}

/// butteraugli: max norm (`butteraugli`) and libjxl 3-norm (`butteraugli/3-norm`).
pub struct Butteraugli;

impl Judge for Butteraugli {
    fn name(&self) -> &'static str {
        "butteraugli"
    }

    fn keys(&self) -> Vec<String> {
        vec!["butteraugli".into(), "butteraugli/3-norm".into()]
    }

    fn score(&self, reference: &Rgb8Image, distorted: &Rgb8Image) -> Res<Vec<Score>> {
        let out = butteraugli::butteraugli(
            reference.imgref(),
            distorted.imgref(),
            &butteraugli::ButteraugliParams::default(),
        )
        .map_err(|e| format!("butteraugli: {e}"))?;
        let tag = |s: Score| {
            s.version(pins::BUTTERAUGLI.version)
                .implementation(pins::BUTTERAUGLI.implementation("butteraugli"))
        };
        Ok(vec![
            tag(Score::new("butteraugli", out.score, Better::Lower)),
            tag(Score::new("butteraugli", out.pnorm_3, Better::Lower).variant("3-norm")),
        ])
    }
}

/// Every judge this binary knows, by name.
pub fn by_name(name: &str) -> Option<Box<dyn Judge>> {
    Some(match name {
        "zensim" => Box::new(Zensim::new()),
        "ssim2" => Box::new(Ssim2),
        "butteraugli" => Box::new(Butteraugli),
        _ => return None,
    })
}

pub const ALL: &[&str] = &["zensim", "ssim2", "butteraugli"];

/// Score keys of a judge list, in order.
pub fn keys(judges: &[Box<dyn Judge>]) -> Vec<String> {
    judges.iter().flat_map(|j| j.keys()).collect()
}

/// Run every judge; the result is in [`keys`] order.
pub fn score_all(
    judges: &[Box<dyn Judge>],
    reference: &Rgb8Image,
    distorted: &Rgb8Image,
) -> Res<Vec<Score>> {
    let mut out = Vec::new();
    for j in judges {
        out.extend(j.score(reference, distorted)?);
    }
    Ok(out)
}
