//! Cool-chic possession, including the trained native stream and its decoded pixels.
use super::{Encoded, Family};
use crate::{Res, image::Rgb8Image, pins, rng::Rng};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zencodec::encode::{EncodeJob, Encoder, EncoderConfig};
use zencoolchic::codec::{Architecture, CoolChicEncoderConfig};
use zenpixels::{PixelDescriptor, PixelSlice};

const IMPLEMENTATION: &str = "zencoolchic@1cecd62037dd8a8fe14a8a46d872ae7165538b33";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    implementation: String,
    lambda: f32,
    iterations: usize,
    architecture: u8,
    seed: u64,
    rdoq: bool,
}
impl Params {
    fn baseline(quality: f64) -> Self {
        Self {
            implementation: IMPLEMENTATION.into(),
            lambda: 10f32.powf(-1.0 - 4.0 * (quality.clamp(0.0, 100.0) as f32 / 100.0)),
            iterations: 2000,
            architecture: 1,
            seed: 0,
            rdoq: true,
        }
    }
    fn validate(&self) -> Res<()> {
        if self.implementation != IMPLEMENTATION {
            return Err("recipe requires a different zencoolchic revision".into());
        }
        if !self.lambda.is_finite()
            || !(0.0..=1.0).contains(&self.lambda)
            || self.lambda == 0.0
            || !(2000..=20000).contains(&self.iterations)
            || self.architecture > 3
        {
            return Err("Cool-chic parameters outside supported ranges".into());
        }
        Ok(())
    }
}
pub(crate) struct Zencoolchic;
impl Family for Zencoolchic {
    fn name(&self) -> &'static str {
        "zencoolchic"
    }
    fn codec(&self) -> Option<(&'static str, &'static pins::Pin)> {
        Some(("zencoolchic", &pins::ZENCOOLCHIC))
    }
    fn baseline(&self, quality: f64) -> Option<Value> {
        Some(serde_json::to_value(Params::baseline(quality)).unwrap())
    }
    fn sample(&self, rng: &mut Rng, _: &Rgb8Image) -> Value {
        let mut p = Params::baseline(rng.range(5.0, 100.0));
        if rng.chance(0.8) {
            p.architecture = rng.below(4) as u8;
        } else {
            p.iterations = *rng.pick(&[2000, 3000, 4000, 6000]);
            p.architecture = rng.below(4) as u8;
            p.rdoq = rng.chance(0.5);
            p.seed = rng.below(16) as u64;
        }
        serde_json::to_value(p).unwrap()
    }
    fn encode(&self, params: &Value, reference: &Rgb8Image) -> Res<Encoded> {
        let p: Params =
            serde_json::from_value(params.clone()).map_err(|e| format!("params: {e}"))?;
        p.validate()?;
        let arch = match p.architecture {
            0 => Architecture::VeryHigh,
            1 => Architecture::High,
            2 => Architecture::Medium,
            _ => Architecture::Low,
        };
        let pixels = PixelSlice::new(
            &reference.data,
            reference.width,
            reference.height,
            reference.stride(),
            PixelDescriptor::RGB8_SRGB,
        )
        .map_err(|e| format!("Cool-chic input: {e}"))?;
        let bytes = CoolChicEncoderConfig::default()
            .with_lambda(p.lambda)
            .with_iterations(p.iterations)
            .with_architecture(arch)
            .with_seed(p.seed)
            .with_rdoq(p.rdoq)
            .job()
            .encoder()
            .map_err(|e| format!("Cool-chic encoder: {e}"))?
            .encode(pixels)
            .map_err(|e| format!("Cool-chic encode: {e}"))?
            .data()
            .to_vec();
        let decoded = zencoolchic::decode(&bytes, &zencoolchic::Limits::default())
            .map_err(|e| format!("Cool-chic decode: {e}"))?;
        let data = decoded
            .to_rgb8()
            .ok_or("Cool-chic decoder did not return RGB8")?;
        let rendition = Rgb8Image::new(decoded.width, decoded.height, data)?;
        if (rendition.width, rendition.height) != (reference.width, reference.height) {
            return Err("Cool-chic decoded to different dimensions".into());
        }
        Ok(Encoded {
            bits: Some(evif::Bitstream {
                codec: *b"cool",
                data: bytes,
            }),
            pixels: rendition,
            claimed_bytes: None,
        })
    }
}
