//! The `corruption-corpus` structural defects (channel swaps, dropped blocks,
//! off-by-one edges, wrong-phase chroma, real decoder bugs) as a synthetic
//! family. No bitstream: a file from this family carries only pixels, and a
//! `clam` size when the caller gives one.

use corruption_corpus::{CorruptionParams, Rgb8, full_catalog};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Encoded, Family};
use crate::image::Rgb8Image;
use crate::rng::Rng;
use crate::{Res, pins};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Params {
    pub corruption: CorruptionParams,
    pub seed: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claimed_bytes: Option<u64>,
}

pub struct Corruption;

impl Family for Corruption {
    fn name(&self) -> &'static str {
        "corruption"
    }

    fn codec(&self) -> Option<(&'static str, &'static pins::Pin)> {
        Some(("corruption-corpus", &pins::CORRUPTION_CORPUS))
    }

    fn sample(&self, rng: &mut Rng, reference: &Rgb8Image) -> Value {
        let catalog = full_catalog();
        let img = Rgb8::from_raw(reference.width, reference.height, reference.data.clone());
        // Skip entries that leave this reference unchanged (a chroma defect
        // on achromatic content, say): they are not distortions.
        for _ in 0..64 {
            let corruption = rng.pick(&catalog).clone();
            let seed = rng.next_u64();
            if !corruption.is_identity_on(&img, seed) {
                return serde_json::to_value(Params {
                    corruption,
                    seed,
                    claimed_bytes: None,
                })
                .expect("Params serializes");
            }
        }
        let corruption = catalog[0].clone();
        serde_json::to_value(Params {
            corruption,
            seed: rng.next_u64(),
            claimed_bytes: None,
        })
        .expect("Params serializes")
    }

    fn baseline(&self, _quality: f64) -> Option<Value> {
        None
    }

    fn encode(&self, params: &Value, reference: &Rgb8Image) -> Res<Encoded> {
        let p: Params =
            serde_json::from_value(params.clone()).map_err(|e| format!("params: {e}"))?;
        let mut img = Rgb8::from_raw(reference.width, reference.height, reference.data.clone());
        p.corruption.apply(&mut img, p.seed);
        let pixels = Rgb8Image::new(reference.width, reference.height, img.into_bytes())?;
        Ok(Encoded {
            bits: None,
            pixels,
            claimed_bytes: p.claimed_bytes,
        })
    }
}

/// Slug of a corruption entry, for file names.
pub fn slug(params: &Value) -> Option<String> {
    serde_json::from_value::<Params>(params.clone())
        .ok()
        .map(|p| p.corruption.slug())
}
