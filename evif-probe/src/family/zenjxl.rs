//! JPEG XL VarDCT possession; every candidate is decoded from its actual bytes.
use super::{Encoded, Family};
use crate::{Res, image::Rgb8Image, pins, rng::Rng};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zenpixels_convert::PixelBufferConvertTypedExt;

const IMPLEMENTATION: &str = "zenjxl@3f5e22e9cf8e2d58311af183d3f20454deeae6be;jxl-encoder@ce4ec7a8e4bf4a74a0b55a1851bc5b2b84167640;zenjxl-decoder@21159c6ba149bc802f0d985851e568976a44da04";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    implementation: String,
    distance: f32,
    effort: u8,
    gaborish: Option<bool>,
    epf: i8,
    resampling: Option<u32>,
    noise: bool,
    faster_decoding: u8,
    ac_quant: Option<f32>,
    info_loss: Option<f32>,
    cfl_two_pass: Option<bool>,
    chromacity_adjustment: Option<bool>,
    small_transforms: Option<bool>,
}
impl Params {
    fn baseline(quality: f64) -> Self {
        Self {
            implementation: IMPLEMENTATION.into(),
            distance: zenjxl::quality_to_distance(quality.clamp(1.0, 99.0) as f32),
            effort: 7,
            gaborish: None,
            epf: -1,
            resampling: None,
            noise: false,
            faster_decoding: 0,
            ac_quant: None,
            info_loss: None,
            cfl_two_pass: None,
            chromacity_adjustment: None,
            small_transforms: None,
        }
    }
    fn validate(&self) -> Res<()> {
        if self.implementation != IMPLEMENTATION {
            return Err("recipe requires different codec revisions".into());
        }
        if !(0.01..=25.0).contains(&self.distance)
            || !(1..=10).contains(&self.effort)
            || !(-1..=3).contains(&self.epf)
            || self.faster_decoding > 4
            || self.resampling.is_some_and(|x| ![1, 2, 4, 8].contains(&x))
            || self.ac_quant.is_some_and(|x| !(0.05..=4.0).contains(&x))
            || self.info_loss.is_some_and(|x| !(0.05..=8.0).contains(&x))
        {
            return Err("JXL possession parameters outside supported ranges".into());
        }
        Ok(())
    }
}
pub(crate) struct Zenjxl;
impl Family for Zenjxl {
    fn name(&self) -> &'static str {
        "zenjxl"
    }
    fn codec(&self) -> Option<(&'static str, &'static pins::Pin)> {
        Some(("zenjxl", &pins::ZENJXL))
    }
    fn baseline(&self, quality: f64) -> Option<Value> {
        Some(serde_json::to_value(Params::baseline(quality)).unwrap())
    }
    fn sample(&self, rng: &mut Rng, _: &Rgb8Image) -> Value {
        let mut p = Params::baseline(rng.range(1.0, 99.0));
        if rng.chance(0.8) {
            match rng.below(4) {
                0 => p.ac_quant = Some(rng.log_range(0.1, 3.0) as f32),
                1 => {
                    p.epf = rng.below(4) as i8;
                    p.gaborish = Some(rng.chance(0.5));
                }
                2 => p.resampling = Some(*rng.pick(&[1, 2, 4, 8])),
                _ => {
                    p.cfl_two_pass = Some(rng.chance(0.5));
                    p.chromacity_adjustment = Some(rng.chance(0.5));
                }
            }
        } else {
            p.effort = *rng.pick(&[3, 5, 7, 9]);
            p.epf = rng.below(4) as i8;
            p.gaborish = Some(rng.chance(0.5));
            p.noise = rng.chance(0.5);
            p.faster_decoding = rng.below(5) as u8;
            p.info_loss = Some(rng.log_range(0.1, 6.0) as f32);
            p.small_transforms = Some(rng.chance(0.5));
            p.resampling = Some(*rng.pick(&[1, 2, 4, 8]));
        }
        serde_json::to_value(p).unwrap()
    }
    fn encode(&self, params: &Value, reference: &Rgb8Image) -> Res<Encoded> {
        let p: Params =
            serde_json::from_value(params.clone()).map_err(|e| format!("params: {e}"))?;
        p.validate()?;
        let mut internal = zenjxl::LossyInternalParams::default();
        internal.k_ac_quant = p.ac_quant;
        internal.k_info_loss_mul_base = p.info_loss;
        internal.cfl_two_pass = p.cfl_two_pass;
        internal.chromacity_adjustment = p.chromacity_adjustment;
        internal.try_dct4x8_afv = p.small_transforms;
        let mut config = zenjxl::LossyConfig::new(p.distance)
            .with_effort(p.effort)
            .with_epf_level(p.epf)
            .with_noise(p.noise)
            .with_faster_decoding(p.faster_decoding)
            .with_internal_params(internal)
            .with_threads(1);
        if let Some(g) = p.gaborish {
            config = config.with_gaborish(g);
        }
        if let Some(r) = p.resampling {
            config = config.with_resampling(r);
        }
        let bytes = zenjxl::encode_rgb8(reference.imgref(), &config)
            .map_err(|e| format!("JXL encode: {e}"))?;
        let decoded = zenjxl::decode(&bytes, None, &[]).map_err(|e| format!("JXL decode: {e}"))?;
        let rgb = decoded.pixels.to_rgb8();
        let pixels = Rgb8Image::new(rgb.width(), rgb.height(), rgb.copy_to_contiguous_bytes())?;
        if (pixels.width, pixels.height) != (reference.width, reference.height) {
            return Err("JXL decoded to different dimensions".into());
        }
        Ok(Encoded {
            bits: Some(evif::Bitstream {
                codec: *b"jxl ",
                data: bytes,
            }),
            pixels,
            claimed_bytes: None,
        })
    }
}
