//! zenjpeg through its `ExpertConfig`: the flat parameter vector with raw
//! per-coefficient quantization tables, zero-bias, adaptive quantization,
//! deringing and trellis. The sampled vector perturbs jpegli's defaults per
//! component and frequency band, so the search can move luma against chroma
//! and low frequencies against high ones, the degrees of freedom a metric
//! has to weigh correctly.

use enough::Unstoppable;
use evif::Bitstream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zenjpeg::decoder::Decoder;
use zenjpeg::encoder::{ChromaSubsampling, ColorMode, ExpertConfig, PixelLayout};

use super::{Encoded, Family, Subsampling};
use crate::image::Rgb8Image;
use crate::rng::Rng;
use crate::{Res, pins};

/// Multipliers on the base quantization table, per component, per band.
/// Band 0 is DC; 1, 2, 3 are the low, mid and high AC bands by zigzag
/// position (1..=9, 10..=35, 36..=63).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Params {
    pub quality: f64,
    pub subsampling: Subsampling,
    /// `[component][band]` multipliers on the base quant matrix.
    pub quant_scale: [[f64; 4]; 3],
    /// Multipliers on the zero-bias tables, per component.
    pub zero_bias_scale: [f64; 3],
    pub adaptive_quant: bool,
    pub deringing: bool,
    pub trellis: bool,
}

impl Params {
    /// jpegli defaults at `quality`, 4:2:0.
    pub fn baseline(quality: f64) -> Self {
        Self {
            quality,
            subsampling: Subsampling::S420,
            quant_scale: [[1.0; 4]; 3],
            zero_bias_scale: [1.0; 3],
            adaptive_quant: true,
            deringing: false,
            trellis: false,
        }
    }
}

pub struct ZenjpegExpert;

/// Zigzag position of each natural-order coefficient index.
const ZIGZAG_POS: [u8; 64] = {
    const ORDER: [u8; 64] = [
        0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27,
        20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51,
        58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
    ];
    let mut pos = [0u8; 64];
    let mut z = 0;
    while z < 64 {
        pos[ORDER[z] as usize] = z as u8;
        z += 1;
    }
    pos
};

fn band(natural_index: usize) -> usize {
    match ZIGZAG_POS[natural_index] {
        0 => 0,
        1..=9 => 1,
        10..=35 => 2,
        _ => 3,
    }
}

fn subsampling(s: Subsampling) -> ChromaSubsampling {
    match s {
        Subsampling::S444 => ChromaSubsampling::None,
        Subsampling::S422 => ChromaSubsampling::HalfHorizontal,
        Subsampling::S440 => ChromaSubsampling::HalfVertical,
        Subsampling::S420 => ChromaSubsampling::Quarter,
    }
}

impl Family for ZenjpegExpert {
    fn name(&self) -> &'static str {
        "zenjpeg-expert"
    }

    fn codec(&self) -> Option<(&'static str, &'static pins::Pin)> {
        Some(("zenjpeg", &pins::ZENJPEG))
    }

    fn sample(&self, rng: &mut Rng, _reference: &Rgb8Image) -> Value {
        let mut p = Params::baseline(rng.range(20.0, 95.0));
        p.subsampling = *rng.pick(&Subsampling::ALL);
        for comp in &mut p.quant_scale {
            for b in comp.iter_mut() {
                // Log-uniform over a factor of 6 each way: wide enough to
                // reach visibly broken tables, centred on the default.
                *b = rng.log_range(1.0 / 6.0, 6.0);
            }
        }
        for z in &mut p.zero_bias_scale {
            *z = rng.log_range(0.25, 4.0);
        }
        p.adaptive_quant = rng.chance(0.7);
        p.deringing = rng.chance(0.3);
        p.trellis = rng.chance(0.3);
        serde_json::to_value(p).expect("Params serializes")
    }

    fn baseline(&self, quality: f64) -> Option<Value> {
        Some(serde_json::to_value(Params::baseline(quality)).expect("Params serializes"))
    }

    fn encode(&self, params: &Value, reference: &Rgb8Image) -> Res<Encoded> {
        let p: Params =
            serde_json::from_value(params.clone()).map_err(|e| format!("params: {e}"))?;
        let mut x = ExpertConfig::default_ycbcr(p.quality as f32);
        for (c, comp) in [
            &mut x.tables.quant.c0,
            &mut x.tables.quant.c1,
            &mut x.tables.quant.c2,
        ]
        .into_iter()
        .enumerate()
        {
            for (i, q) in comp.iter_mut().enumerate() {
                *q *= p.quant_scale[c][band(i)] as f32;
            }
        }
        for (c, comp) in [
            &mut x.tables.zero_bias_mul.c0,
            &mut x.tables.zero_bias_mul.c1,
            &mut x.tables.zero_bias_mul.c2,
        ]
        .into_iter()
        .enumerate()
        {
            for z in comp.iter_mut() {
                *z *= p.zero_bias_scale[c] as f32;
            }
        }
        x.aq_enabled = p.adaptive_quant;
        x.deringing = p.deringing;
        x.trellis_enabled = p.trellis;
        let config = x.to_encoder_config(ColorMode::YCbCr {
            subsampling: subsampling(p.subsampling),
        });

        let mut enc = config
            .encode_from_bytes(reference.width, reference.height, PixelLayout::Rgb8Srgb)
            .map_err(|e| format!("zenjpeg config: {e}"))?;
        enc.push_packed(&reference.data, Unstoppable)
            .map_err(|e| format!("zenjpeg encode: {e}"))?;
        let jpeg = enc.finish().map_err(|e| format!("zenjpeg finish: {e}"))?;

        let dec = Decoder::new()
            .decode(&jpeg, Unstoppable)
            .map_err(|e| format!("zenjpeg decode: {e}"))?;
        let (w, h) = dec.dimensions();
        let px = dec
            .pixels_u8()
            .ok_or("zenjpeg decode produced no u8 pixels")?;
        if (w, h) != (reference.width, reference.height) {
            return Err(format!(
                "decoded {w}x{h}, reference {}x{}",
                reference.width, reference.height
            ));
        }
        let pixels = Rgb8Image::new(w, h, px.to_vec())?;
        Ok(Encoded {
            bits: Some(Bitstream {
                codec: *b"jpeg",
                data: jpeg,
            }),
            pixels,
            claimed_bytes: None,
        })
    }
}
