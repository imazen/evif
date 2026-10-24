//! Stress families: ways of producing a distorted rendition of a reference,
//! each with a parameter vector the search can sample and the file records.

use evif::Bitstream;
use serde::{Deserialize, Serialize};

use crate::Res;
use crate::image::Rgb8Image;
use crate::rng::Rng;

pub mod corruption;
pub mod zenjpeg_expert;

/// What one parameter vector produced.
pub struct Encoded {
    /// The interior bitstream, when the family is a real codec.
    pub bits: Option<Bitstream>,
    /// Pixels our decoder produced from `bits` (or the synthetic output).
    pub pixels: Rgb8Image,
    /// A synthetic family's pretend size, for the `clam` chunk.
    pub claimed_bytes: Option<u64>,
}

impl Encoded {
    pub fn bytes(&self) -> Option<u64> {
        self.bits
            .as_ref()
            .map(|b| b.data.len() as u64)
            .or(self.claimed_bytes)
    }

    pub fn bpp(&self) -> Option<f64> {
        self.bytes()
            .map(|b| b as f64 * 8.0 / self.pixels.pixels() as f64)
    }
}

pub trait Family {
    /// Family name as written in `knob.family`.
    fn name(&self) -> &'static str;
    /// Codec identity for `knob`, if the family is a real codec.
    fn codec(&self) -> Option<(&'static str, &'static crate::pins::Pin)>;
    /// Draw one parameter vector.
    fn sample(&self, rng: &mut Rng, reference: &Rgb8Image) -> serde_json::Value;
    /// The family's honest baseline at a given quality in `0..=100`, used to
    /// build the rate-distortion curve the search compares against. `None`
    /// when the family has no quality axis (synthetic distortions).
    fn baseline(&self, quality: f64) -> Option<serde_json::Value>;
    /// Produce the rendition for one parameter vector.
    fn encode(&self, params: &serde_json::Value, reference: &Rgb8Image) -> Res<Encoded>;
}

pub fn by_name(name: &str) -> Option<Box<dyn Family>> {
    Some(match name {
        "zenjpeg-expert" => Box::new(zenjpeg_expert::ZenjpegExpert),
        "corruption" => Box::new(corruption::Corruption),
        _ => return None,
    })
}

pub const ALL: &[&str] = &["zenjpeg-expert", "corruption"];

/// Chroma subsampling choices shared by the JPEG-shaped families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Subsampling {
    S444,
    S422,
    S440,
    S420,
}

impl Subsampling {
    pub const ALL: [Self; 4] = [Self::S444, Self::S422, Self::S440, Self::S420];
}
