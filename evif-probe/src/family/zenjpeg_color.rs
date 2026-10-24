//! zenjpeg's color-side knobs through the public builder: color path (YCbCr,
//! XYB, RGB passthrough), the quantization-table family with its chroma
//! control (jpegli chroma distance scales, mozjpeg chroma quality), chroma
//! downsampling method, deringing, adaptive quantization and auto-optimize.
//! Where `zenjpeg-expert` bends individual table entries, this family moves
//! the color pipeline as a whole, the part a luma-weighted metric is most
//! likely to under-weigh.

use enough::Unstoppable;
use evif::Bitstream;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zenjpeg::encoder::{
    ChromaSubsampling, DownsamplingMethod, EncoderConfig, PixelLayout, QuantTableConfig,
    XybSubsampling,
};

use super::{Encoded, Family, Subsampling, decode_jpeg};
use crate::image::Rgb8Image;
use crate::rng::Rng;
use crate::{Res, pins};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "path")]
pub enum ColorPath {
    Ycbcr {
        subsampling: Subsampling,
    },
    Xyb {
        b_quarter: bool,
    },
    /// R, G, B coded as three independent planes, no color transform.
    Rgb,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "family")]
pub enum Tables {
    /// jpegli 3-table with per-channel chroma distance multipliers
    /// (`[Cb, Cr]` or `[X, B]`), each in `0.1..=5`.
    Jpegli { chroma_scales: [f64; 2] },
    /// jpegli 2-table (shared chroma). YCbCr only.
    JpegliShared { chroma_scales: [f64; 2] },
    /// mozjpeg Robidoux tables with an independent chroma quality. YCbCr only.
    Mozjpeg { chroma_quality: Option<u8> },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Downsampling {
    Box,
    GammaAware,
    GammaAwareIterative,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Params {
    pub quality: f64,
    pub color: ColorPath,
    pub tables: Tables,
    pub downsampling: Downsampling,
    pub deringing: bool,
    pub adaptive_quant: bool,
    pub auto_optimize: bool,
}

impl Params {
    pub fn baseline(quality: f64) -> Self {
        Self {
            quality,
            color: ColorPath::Ycbcr {
                subsampling: Subsampling::S420,
            },
            tables: Tables::Jpegli {
                chroma_scales: [1.0, 1.0],
            },
            downsampling: Downsampling::Box,
            deringing: false,
            adaptive_quant: true,
            auto_optimize: false,
        }
    }
}

pub struct ZenjpegColor;

fn subsampling(s: Subsampling) -> ChromaSubsampling {
    match s {
        Subsampling::S444 => ChromaSubsampling::None,
        Subsampling::S422 => ChromaSubsampling::HalfHorizontal,
        Subsampling::S440 => ChromaSubsampling::HalfVertical,
        Subsampling::S420 => ChromaSubsampling::Quarter,
    }
}

impl Family for ZenjpegColor {
    fn name(&self) -> &'static str {
        "zenjpeg-color"
    }

    fn codec(&self) -> Option<(&'static str, &'static pins::Pin)> {
        Some(("zenjpeg", &pins::ZENJPEG))
    }

    fn sample(&self, rng: &mut Rng, _reference: &Rgb8Image) -> Value {
        let mut p = Params::baseline(rng.range(20.0, 95.0));
        p.color = match rng.below(3) {
            0 => ColorPath::Ycbcr {
                subsampling: *rng.pick(&Subsampling::ALL),
            },
            1 => ColorPath::Xyb {
                b_quarter: rng.chance(0.5),
            },
            _ => ColorPath::Rgb,
        };
        let scales = [rng.log_range(0.1, 5.0), rng.log_range(0.1, 5.0)];
        p.tables = match p.color {
            // XYB and RGB accept only the 3-table jpegli layout.
            ColorPath::Xyb { .. } | ColorPath::Rgb => Tables::Jpegli {
                chroma_scales: scales,
            },
            ColorPath::Ycbcr { .. } => match rng.below(3) {
                0 => Tables::Jpegli {
                    chroma_scales: scales,
                },
                1 => Tables::JpegliShared {
                    chroma_scales: scales,
                },
                _ => Tables::Mozjpeg {
                    chroma_quality: rng.chance(0.8).then(|| (rng.range(1.0, 100.0)) as u8),
                },
            },
        };
        p.downsampling = *rng.pick(&[
            Downsampling::Box,
            Downsampling::GammaAware,
            Downsampling::GammaAwareIterative,
        ]);
        p.deringing = rng.chance(0.3);
        p.adaptive_quant = rng.chance(0.7);
        p.auto_optimize = rng.chance(0.3);
        serde_json::to_value(p).expect("Params serializes")
    }

    fn baseline(&self, quality: f64) -> Option<Value> {
        Some(serde_json::to_value(Params::baseline(quality)).expect("Params serializes"))
    }

    fn encode(&self, params: &Value, reference: &Rgb8Image) -> Res<Encoded> {
        let p: Params =
            serde_json::from_value(params.clone()).map_err(|e| format!("params: {e}"))?;
        let q = p.quality as f32;
        let cfg = match p.color {
            ColorPath::Ycbcr { subsampling: s } => EncoderConfig::ycbcr(q, subsampling(s)),
            ColorPath::Xyb { b_quarter } => EncoderConfig::xyb(
                q,
                if b_quarter {
                    XybSubsampling::BQuarter
                } else {
                    XybSubsampling::Full
                },
            ),
            ColorPath::Rgb => EncoderConfig::rgb(q),
        };
        let tables = match p.tables {
            Tables::Jpegli { chroma_scales: s } => QuantTableConfig::Jpegli {
                chroma_distance_scales: [s[0] as f32, s[1] as f32],
            },
            Tables::JpegliShared { chroma_scales: s } => QuantTableConfig::JpegliSharedChroma {
                chroma_distance_scales: [s[0] as f32, s[1] as f32],
            },
            Tables::Mozjpeg { chroma_quality } => {
                QuantTableConfig::MozjpegRobidoux { chroma_quality }
            }
        };
        let cfg = cfg
            .quant_table_config(tables)
            .downsampling_method(match p.downsampling {
                Downsampling::Box => DownsamplingMethod::Box,
                Downsampling::GammaAware => DownsamplingMethod::GammaAware,
                Downsampling::GammaAwareIterative => DownsamplingMethod::GammaAwareIterative,
            })
            .deringing(p.deringing)
            .aq_enabled(p.adaptive_quant)
            .auto_optimize(p.auto_optimize);

        let mut enc = cfg
            .encode_from_bytes(reference.width, reference.height, PixelLayout::Rgb8Srgb)
            .map_err(|e| format!("zenjpeg config: {e}"))?;
        enc.push_packed(&reference.data, Unstoppable)
            .map_err(|e| format!("zenjpeg encode: {e}"))?;
        let jpeg = enc.finish().map_err(|e| format!("zenjpeg finish: {e}"))?;
        let pixels = decode_jpeg(&jpeg, reference)?;
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
