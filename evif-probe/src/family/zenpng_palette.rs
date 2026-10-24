//! Lossy PNG: zenpng writing an indexed image from a zenquant palette.
//! Palette size, quality preset, dither mode and strength, and the Viterbi
//! run-extension weight decide how the error is spread: banding in smooth
//! regions, dither noise in texture, lost hues when the palette is small.
//! Each of those is a different thing for a metric to get wrong.

use enough::Unstoppable;
use evif::Bitstream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zenquant::{OutputFormat, QuantizeConfig};

use super::{Encoded, Family};
use crate::image::Rgb8Image;
use crate::rng::Rng;
use crate::{Res, pins};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset {
    Fast,
    Balanced,
    Best,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Dither {
    None,
    Adaptive,
    BlueNoise,
    SierraLite,
    Linear,
    Ordered,
}

impl Dither {
    const ALL: [Self; 6] = [
        Self::None,
        Self::Adaptive,
        Self::BlueNoise,
        Self::SierraLite,
        Self::Linear,
        Self::Ordered,
    ];
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Params {
    /// Palette entries, `2..=256`.
    pub max_colors: u32,
    pub preset: Preset,
    pub dither: Dither,
    /// Error-diffusion strength override, `0..=1`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dither_strength: Option<f64>,
    /// Viterbi run-extension weight override.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub viterbi_lambda: Option<f64>,
}

impl Params {
    /// Quality `0..=100` maps log-linearly onto `2..=256` colors, with
    /// zenquant's defaults for everything else.
    pub fn baseline(quality: f64) -> Self {
        let t = (quality / 100.0).clamp(0.0, 1.0);
        let colors = (2.0_f64.powf(1.0 + 7.0 * t)).round() as u32;
        Self {
            max_colors: colors.clamp(2, 256),
            preset: Preset::Best,
            dither: Dither::Adaptive,
            dither_strength: None,
            viterbi_lambda: None,
        }
    }
}

pub struct ZenpngPalette;

impl Family for ZenpngPalette {
    fn name(&self) -> &'static str {
        "zenpng-palette"
    }

    fn codec(&self) -> Option<(&'static str, &'static pins::Pin)> {
        Some(("zenpng+zenquant", &pins::ZENPNG))
    }

    fn sample(&self, rng: &mut Rng, _reference: &Rgb8Image) -> Value {
        let mut p = Params::baseline(rng.range(0.0, 100.0));
        p.preset = *rng.pick(&[Preset::Fast, Preset::Balanced, Preset::Best]);
        p.dither = *rng.pick(&Dither::ALL);
        p.dither_strength = rng.chance(0.5).then(|| rng.range(0.0, 1.0));
        p.viterbi_lambda = rng.chance(0.3).then(|| rng.log_range(0.01, 10.0));
        serde_json::to_value(p).expect("Params serializes")
    }

    fn baseline(&self, quality: f64) -> Option<Value> {
        Some(serde_json::to_value(Params::baseline(quality)).expect("Params serializes"))
    }

    fn encode(&self, params: &Value, reference: &Rgb8Image) -> Res<Encoded> {
        let p: Params =
            serde_json::from_value(params.clone()).map_err(|e| format!("params: {e}"))?;
        if !(2..=256).contains(&p.max_colors) {
            return Err(format!("max_colors {} out of 2..=256", p.max_colors));
        }
        let mut cfg = QuantizeConfig::new(OutputFormat::Png)
            .with_max_colors(p.max_colors)
            .with_quality(match p.preset {
                Preset::Fast => zenquant::Quality::Fast,
                Preset::Balanced => zenquant::Quality::Balanced,
                Preset::Best => zenquant::Quality::Best,
            });
        cfg = match p.dither {
            Dither::None => cfg._with_no_dither(),
            Dither::Adaptive => cfg._with_adaptive_dither(),
            Dither::BlueNoise => cfg._with_blue_noise_dither(),
            Dither::SierraLite => cfg._with_sierra_lite_dither(),
            Dither::Linear => cfg._with_linear_dither(),
            Dither::Ordered => cfg._with_ordered_dither(),
        };
        if let Some(s) = p.dither_strength {
            cfg = cfg._with_dither_strength(s as f32);
        }
        if let Some(l) = p.viterbi_lambda {
            cfg = cfg._with_viterbi_lambda(l as f32);
        }
        let quantizer = zenpng::ZenquantQuantizer::from_config(cfg);

        let rgba: Vec<rgb::RGBA8> = reference
            .triples()
            .iter()
            .map(|&[r, g, b]| rgb::RGBA8::new(r, g, b, 255))
            .collect();
        let img = imgref::ImgRef::new(&rgba, reference.width as usize, reference.height as usize);
        let png = zenpng::encode_indexed(
            img,
            &zenpng::EncodeConfig::default(),
            &quantizer,
            None,
            &Unstoppable,
            &Unstoppable,
        )
        .map_err(|e| format!("zenpng indexed encode: {e}"))?;
        let pixels = Rgb8Image::from_png(&png)?;
        if (pixels.width, pixels.height) != (reference.width, reference.height) {
            return Err("indexed PNG decoded to different dimensions".into());
        }
        Ok(Encoded {
            bits: Some(Bitstream {
                codec: *b"png ",
                data: png,
            }),
            pixels,
            claimed_bytes: None,
        })
    }
}
