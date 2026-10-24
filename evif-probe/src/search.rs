//! Matched-bitrate disagreement search.
//!
//! For each reference the family's honest baseline is swept over quality to
//! give a rate-distortion curve per score key. Every sampled parameter vector
//! is then scored, and its gain on each key is its score minus the baseline
//! curve's value at the same bits per pixel, in units of that key's baseline
//! range (so a gain of 0.1 means a tenth of the span between the worst and
//! best honest encode). The target's gain minus the mean judge gain is the
//! disagreement: how much the target likes this encode more than the others
//! do, for the bytes it costs.
//!
//! This is the maximum-differentiation idea (Wang & Simoncelli 2008) run as
//! a black-box search over codec knobs instead of pixel gradients.

use std::collections::BTreeMap;

use evif::Score;

use crate::Res;
use crate::family::{Encoded, Family};
use crate::image::Rgb8Image;
use crate::judge::{self, Judge};
use crate::rng::Rng;

/// One scored encode.
#[derive(Debug, Clone)]
pub struct Sample {
    pub params: serde_json::Value,
    pub bpp: Option<f64>,
    pub scores: Vec<Score>,
    /// Gain per score key against the baseline curve at this bpp, in units
    /// of the key's baseline range. Empty when there is no baseline.
    pub gain: BTreeMap<String, f64>,
    pub disagreement: Option<f64>,
}

/// The honest curve: baseline encodes at a quality grid, sorted by bpp.
pub struct Baseline {
    /// `(bpp, score per key)` ascending in bpp.
    points: Vec<(f64, BTreeMap<String, f64>)>,
    /// Per key: `(min, max)` of the baseline scores.
    range: BTreeMap<String, (f64, f64)>,
}

impl Baseline {
    pub fn build(
        family: &dyn Family,
        judges: &[Box<dyn Judge>],
        reference: &Rgb8Image,
        qualities: &[f64],
    ) -> Res<Option<Self>> {
        let mut points = Vec::new();
        for &q in qualities {
            let Some(params) = family.baseline(q) else {
                return Ok(None);
            };
            let enc = family.encode(&params, reference)?;
            let Some(bpp) = enc.bpp() else {
                return Ok(None);
            };
            let scores = judge::score_all(judges, reference, &enc.pixels)?;
            points.push((bpp, by_key(&scores)));
        }
        points.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut range: BTreeMap<String, (f64, f64)> = BTreeMap::new();
        for (_, s) in &points {
            for (k, v) in s {
                let e = range.entry(k.clone()).or_insert((*v, *v));
                e.0 = e.0.min(*v);
                e.1 = e.1.max(*v);
            }
        }
        Ok(Some(Self { points, range }))
    }

    /// Baseline score for `key` at `bpp`, linearly interpolated; clamped to
    /// the ends of the curve outside the swept range.
    pub fn at(&self, key: &str, bpp: f64) -> Option<f64> {
        let first = self.points.first()?;
        if bpp <= first.0 {
            return first.1.get(key).copied();
        }
        for w in self.points.windows(2) {
            let (b0, s0) = (&w[0].0, &w[0].1);
            let (b1, s1) = (&w[1].0, &w[1].1);
            if bpp <= *b1 {
                let (v0, v1) = (*s0.get(key)?, *s1.get(key)?);
                let t = if b1 > b0 { (bpp - b0) / (b1 - b0) } else { 0.0 };
                return Some(v0 + (v1 - v0) * t);
            }
        }
        self.points.last()?.1.get(key).copied()
    }

    /// Honest-curve span of `key`, or `None` when the curve is flat.
    pub fn span(&self, key: &str) -> Option<f64> {
        let (lo, hi) = self.range.get(key)?;
        (hi > lo).then(|| hi - lo)
    }

    pub fn points(&self) -> &[(f64, BTreeMap<String, f64>)] {
        &self.points
    }
}

pub fn by_key(scores: &[Score]) -> BTreeMap<String, f64> {
    scores.iter().map(|s| (s.key(), s.value)).collect()
}

/// Signed gain in the "more similar" direction.
fn signed(score: &Score, baseline: f64, span: f64) -> f64 {
    let d = score.value - baseline;
    let d = match score.better {
        evif::Better::Higher => d,
        evif::Better::Lower => -d,
    };
    d / span
}

/// Everything a search needs besides the random stream: one reference, the
/// family under test, the judges, the honest curve and the score key being
/// optimized.
pub struct Setup<'a> {
    pub family: &'a dyn Family,
    pub judges: &'a [Box<dyn Judge>],
    pub baseline: Option<&'a Baseline>,
    pub reference: &'a Rgb8Image,
    pub target: &'a str,
}

impl Setup<'_> {
    /// Score one encode and compute its gains and disagreement.
    pub fn evaluate(&self, params: serde_json::Value, enc: &Encoded) -> Res<Sample> {
        let scores = judge::score_all(self.judges, self.reference, &enc.pixels)?;
        let bpp = enc.bpp();
        let mut gain = BTreeMap::new();
        if let (Some(b), Some(bpp)) = (self.baseline, bpp) {
            for s in &scores {
                let key = s.key();
                if let (Some(base), Some(span)) = (b.at(&key, bpp), b.span(&key)) {
                    gain.insert(key, signed(s, base, span));
                }
            }
        }
        let disagreement = gain.get(self.target).and_then(|t| {
            let others: Vec<f64> = gain
                .iter()
                .filter(|(k, _)| k.as_str() != self.target)
                .map(|(_, v)| *v)
                .collect();
            (!others.is_empty()).then(|| t - others.iter().sum::<f64>() / others.len() as f64)
        });
        Ok(Sample {
            params,
            bpp,
            scores,
            gain,
            disagreement,
        })
    }

    /// Draw and evaluate `count` encodable samples. A parameter vector the
    /// family refuses is reported to `on_skip` and replaced by a fresh draw;
    /// more refusals than four times `count` is an error, since the family
    /// is then sampling outside what it can encode.
    pub fn run(
        &self,
        rng: &mut Rng,
        count: usize,
        mut on_sample: impl FnMut(usize, &Sample, &Encoded),
        mut on_skip: impl FnMut(usize, &str),
    ) -> Res<Vec<(Sample, Encoded)>> {
        let mut out = Vec::with_capacity(count);
        let mut skipped = 0usize;
        while out.len() < count {
            let params = self.family.sample(rng, self.reference);
            let enc = match self.family.encode(&params, self.reference) {
                Ok(enc) => enc,
                Err(why) => {
                    skipped += 1;
                    on_skip(out.len(), &why);
                    if skipped > count * 4 {
                        return Err(format!(
                            "{} refused {skipped} parameter vectors for {count} samples; last: {why}",
                            self.family.name()
                        ));
                    }
                    continue;
                }
            };
            let sample = self.evaluate(params, &enc)?;
            on_sample(out.len(), &sample, &enc);
            out.push((sample, enc));
        }
        Ok(out)
    }
}
