//! SVT still-image possession through zenavif's pinned RGB/AVIF seam.
use super::{Encoded, Family};
use crate::{Res, image::Rgb8Image, pins, rng::Rng};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zenpixels_convert::PixelBufferConvertTypedExt;

const IMPLEMENTATION: &str = "zenavif@a7638f6e0655914fb15fe4633ac26425533756c8;zenav1-svt@0f63ca13757eea3075f29656b7886143073d2022;rav1d-safe@dfd01e3268f6e8da862d2d5f0a71d19f414f2b87";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    implementation: String,
    quality: f32,
    speed: u8,
    tune: u8,
    variance_boost: bool,
    variance_strength: u8,
    variance_octile: u8,
    qm: bool,
    qm_min: u8,
    qm_max: u8,
    sharpness: i8,
    screen_content: Option<u8>,
    ac_bias: f64,
    max_tx_size: u8,
    chroma_q: Option<(i8, i8)>,
}
impl Params {
    fn baseline(quality: f64) -> Self {
        Self {
            implementation: IMPLEMENTATION.into(),
            quality: quality.clamp(1.0, 100.0) as f32,
            speed: 6,
            tune: 1,
            variance_boost: false,
            variance_strength: 2,
            variance_octile: 5,
            qm: false,
            qm_min: 8,
            qm_max: 15,
            sharpness: 0,
            screen_content: None,
            ac_bias: 0.0,
            max_tx_size: 64,
            chroma_q: None,
        }
    }
    fn validate(&self) -> Res<()> {
        if self.implementation != IMPLEMENTATION {
            return Err("recipe requires different codec revisions".into());
        }
        if !(1.0..=100.0).contains(&self.quality)
            || !(1..=6).contains(&self.speed)
            || self.tune > 4
            || !(1..=4).contains(&self.variance_strength)
            || !(1..=8).contains(&self.variance_octile)
            || self.qm_min > self.qm_max
            || self.qm_max > 15
            || !(0..=7).contains(&self.sharpness)
            || !matches!(self.screen_content, None | Some(3))
            || !(0.0..=8.0).contains(&self.ac_bias)
            || ![32, 64].contains(&self.max_tx_size)
            || self
                .chroma_q
                .is_some_and(|(u, v)| !(-64..=63).contains(&u) || !(-64..=63).contains(&v))
        {
            return Err("SVT possession parameters outside supported ranges".into());
        }
        Ok(())
    }
}
pub(crate) struct Zenav1Svt;
impl Family for Zenav1Svt {
    fn name(&self) -> &'static str {
        "zenav1-svt"
    }
    fn codec(&self) -> Option<(&'static str, &'static pins::Pin)> {
        Some(("zenavif/zenav1-svt", &pins::ZENAVIF_SVT))
    }
    fn baseline(&self, quality: f64) -> Option<Value> {
        Some(serde_json::to_value(Params::baseline(quality)).unwrap())
    }
    fn sample(&self, rng: &mut Rng, _: &Rgb8Image) -> Value {
        let mut p = Params::baseline(rng.range(1.0, 100.0));
        // Most draws isolate likely metric stressors; the tail combines controls.
        if rng.chance(0.8) {
            match rng.below(4) {
                0 => {
                    p.chroma_q = Some((
                        *rng.pick(&[-64, -32, 0, 32, 63]),
                        *rng.pick(&[-64, -32, 0, 32, 63]),
                    ))
                }
                1 => {
                    p.qm = true;
                    p.qm_min = rng.below(16) as u8;
                    p.qm_max = p.qm_min;
                }
                2 => {
                    p.variance_boost = true;
                    p.variance_strength = 1 + rng.below(4) as u8;
                    p.variance_octile = 1 + rng.below(8) as u8;
                }
                _ => {
                    p.sharpness = rng.below(8) as i8;
                    p.ac_bias = rng.range(0.0, 8.0);
                }
            }
        } else {
            p.speed = *rng.pick(&[2, 4, 6]);
            p.tune = rng.below(5) as u8;
            p.qm = rng.chance(0.5);
            p.variance_boost = rng.chance(0.5);
            p.variance_strength = 1 + rng.below(4) as u8;
            p.variance_octile = 1 + rng.below(8) as u8;
            p.sharpness = rng.below(8) as i8;
            p.screen_content = rng.chance(0.5).then_some(3);
            p.max_tx_size = *rng.pick(&[32, 64]);
            p.ac_bias = rng.range(0.0, 8.0);
            p.chroma_q = rng
                .chance(0.5)
                .then(|| (*rng.pick(&[-64, 0, 63]), *rng.pick(&[-64, 0, 63])));
        }
        serde_json::to_value(p).unwrap()
    }
    fn encode(&self, params: &Value, reference: &Rgb8Image) -> Res<Encoded> {
        let p: Params =
            serde_json::from_value(params.clone()).map_err(|e| format!("params: {e}"))?;
        p.validate()?;
        let mut knobs = zenavif::expert::SvtParams::default();
        knobs.tune = p.tune;
        knobs.enable_variance_boost = p.variance_boost;
        knobs.variance_boost_strength = p.variance_strength;
        knobs.variance_octile = p.variance_octile;
        knobs.enable_qm = p.qm;
        knobs.min_qm_level = p.qm_min;
        knobs.max_qm_level = p.qm_max;
        knobs.sharpness = p.sharpness;
        knobs.force_screen_content_mode = p.screen_content;
        knobs.ac_bias = p.ac_bias;
        knobs.max_tx_size = p.max_tx_size;
        knobs.chroma_q = p.chroma_q;
        let config = zenavif::EncoderConfig::new()
            .backend(zenavif::Av1Backend::Zenav1Svt)
            .quality(p.quality)
            .speed(p.speed)
            .bit_depth(zenavif::EncodeBitDepth::Eight)
            .chroma_subsampling(zenavif::EncodeChromaSubsampling::Yuv420)
            .threads(Some(1))
            .with_svt_params(knobs);
        let bytes = zenavif::encode_rgb8(
            reference.imgref(),
            &config,
            almost_enough::StopToken::new(enough::Unstoppable),
        )
        .map_err(|e| format!("SVT encode: {e}"))?
        .avif_file;
        let decoded = zenavif::decode(&bytes).map_err(|e| format!("AVIF decode: {e}"))?;
        let rgb = decoded.to_rgb8();
        let pixels = Rgb8Image::new(rgb.width(), rgb.height(), rgb.copy_to_contiguous_bytes())?;
        if (pixels.width, pixels.height) != (reference.width, reference.height) {
            return Err("AVIF decoded to different dimensions".into());
        }
        Ok(Encoded {
            bits: Some(evif::Bitstream {
                codec: *b"avif",
                data: bytes,
            }),
            pixels,
            claimed_bytes: None,
        })
    }
}
