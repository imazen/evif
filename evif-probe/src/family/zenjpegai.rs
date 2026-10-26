//! JPEG AI possession. The model directory is supplied by the caller's environment.
use super::{Encoded, Family};
use crate::{Res, image::Rgb8Image, pins, rng::Rng};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const IMPLEMENTATION: &str = "zenjpegai@1192fca6b6c2fdc4dcf29a6c8064c53a987caf59";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    implementation: String,
    model_id: u8,
    beta_displacement: i32,
    residual_variance: bool,
    channel_gain: bool,
    latent_scaling: bool,
}

impl Params {
    fn baseline(quality: f64) -> Self {
        Self {
            implementation: IMPLEMENTATION.into(),
            model_id: 1,
            // The native control is signed Q7 sigma-index displacement, not a
            // calibrated generic quality. Cover aggressive and gentle rates.
            beta_displacement: ((quality.clamp(5.0, 100.0) - 50.0) * 10.0) as i32,
            residual_variance: false,
            channel_gain: false,
            latent_scaling: false,
        }
    }
    fn validate(&self) -> Res<()> {
        if self.implementation != IMPLEMENTATION {
            return Err("recipe requires a different zenjpegai revision".into());
        }
        if self.model_id > 3 || !(-1069..=702).contains(&self.beta_displacement) {
            return Err("JPEG AI parameters outside supported ranges".into());
        }
        Ok(())
    }
}

pub(crate) struct Zenjpegai;
impl Family for Zenjpegai {
    fn name(&self) -> &'static str {
        "zenjpegai"
    }
    fn codec(&self) -> Option<(&'static str, &'static pins::Pin)> {
        Some(("zenjpegai", &pins::ZENJPEGAI))
    }
    fn baseline(&self, quality: f64) -> Option<Value> {
        Some(serde_json::to_value(Params::baseline(quality)).unwrap())
    }
    fn sample(&self, rng: &mut Rng, _: &Rgb8Image) -> Value {
        let mut p = Params::baseline(rng.range(5.0, 100.0));
        if rng.chance(0.8) {
            match rng.below(3) {
                0 => p.model_id = rng.below(4) as u8,
                1 => p.residual_variance = true,
                _ => p.channel_gain = true,
            }
        } else {
            p.model_id = rng.below(4) as u8;
            p.residual_variance = rng.chance(0.5);
            p.channel_gain = rng.chance(0.5);
            p.latent_scaling = rng.chance(0.5);
        }
        serde_json::to_value(p).unwrap()
    }
    fn encode(&self, params: &Value, reference: &Rgb8Image) -> Res<Encoded> {
        let p: Params =
            serde_json::from_value(params.clone()).map_err(|e| format!("params: {e}"))?;
        p.validate()?;
        let models = std::env::var_os("ZENJPEGAI_MODELS")
            .ok_or("set ZENJPEGAI_MODELS to the reference checkpoint directory")?;
        let src = zenjpegai::RgbImage {
            width: reference.width as usize,
            height: reference.height as usize,
            bit_depth: 8,
            data: reference.data.iter().map(|&v| u16::from(v)).collect(),
        };
        let knobs = zenjpegai::EncodeParams {
            model_id: p.model_id,
            beta_displacement_log: [p.beta_displacement; 2],
            rvs: p.residual_variance,
            grfs: p.channel_gain,
            lsbs: p.latent_scaling,
            ..Default::default()
        };
        let bytes = zenjpegai::Encoder::new(&models)
            .encode(&src, knobs)
            .map_err(|e| format!("JPEG AI encode: {e}"))?;
        let decoded = zenjpegai::Decoder::new(&models)
            .decode(&bytes)
            .map_err(|e| format!("JPEG AI decode: {e}"))?;
        if decoded.bit_depth != 8 || decoded.data.iter().any(|&v| v > 255) {
            return Err("JPEG AI decoder did not return RGB8".into());
        }
        let pixels = Rgb8Image::new(
            u32::try_from(decoded.width).map_err(|e| e.to_string())?,
            u32::try_from(decoded.height).map_err(|e| e.to_string())?,
            decoded.data.into_iter().map(|v| v as u8).collect(),
        )?;
        if (pixels.width, pixels.height) != (reference.width, reference.height) {
            return Err("JPEG AI decoded to different dimensions".into());
        }
        Ok(Encoded {
            bits: Some(evif::Bitstream {
                codec: *b"jaic",
                data: bytes,
            }),
            pixels,
            claimed_bytes: None,
        })
    }
}
