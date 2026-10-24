//! Matched-bitrate disagreement search.
//!
//! For each reference the family's honest baseline is swept over quality to
//! give a rate-distortion curve per score key. Every sampled parameter vector
//! is then scored, and each key's score is read back through that curve into
//! the bits per pixel an honest encode would need to score the same: the
//! encode's *equivalent bpp* in that metric's eyes. The gain on a key is
//! `log2(equivalent bpp / actual bpp)`: +1 means the metric rates the encode
//! like an honest file twice its size, -1 like one half its size. Outside the
//! swept range the equivalent clamps to the curve's ends, so a catastrophic
//! encode scores the same in every metric instead of being amplified by the
//! metric with the narrowest range. The target's gain minus the mean judge
//! gain is the disagreement, in the same log2-bytes unit.
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
    /// Per score key, `log2(equivalent bpp / actual bpp)` against the
    /// baseline curve. Empty when there is no baseline.
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

    /// The bpp at which the honest curve reaches `value` on `key`, by linear
    /// interpolation on the first segment that crosses it; clamped to the
    /// curve's ends when the value lies outside everything the sweep reached.
    /// `better` says which way along the curve "more similar" lies, so a
    /// value past the best honest score maps to the largest bpp whichever
    /// direction the key runs.
    pub fn equivalent_bpp(&self, key: &str, value: f64, better: evif::Better) -> Option<f64> {
        let first = self.points.first()?;
        let last = self.points.last()?;
        let (v_first, v_last) = (*first.1.get(key)?, *last.1.get(key)?);
        // Orient so that `more` means more similar.
        let more = |a: f64, b: f64| match better {
            evif::Better::Higher => a > b,
            evif::Better::Lower => a < b,
        };
        if more(value, v_last) || value == v_last {
            return Some(last.0);
        }
        if more(v_first, value) || value == v_first {
            return Some(first.0);
        }
        for w in self.points.windows(2) {
            let (b0, v0) = (w[0].0, *w[0].1.get(key)?);
            let (b1, v1) = (w[1].0, *w[1].1.get(key)?);
            let between = (v0 <= value && value <= v1) || (v1 <= value && value <= v0);
            if between && v1 != v0 {
                return Some(b0 + (b1 - b0) * (value - v0) / (v1 - v0));
            }
        }
        // The curve is not monotone and no segment crosses: nearest end.
        Some(if (value - v_first).abs() < (value - v_last).abs() {
            first.0
        } else {
            last.0
        })
    }

    pub fn points(&self) -> &[(f64, BTreeMap<String, f64>)] {
        &self.points
    }
}

pub fn by_key(scores: &[Score]) -> BTreeMap<String, f64> {
    scores.iter().map(|s| (s.key(), s.value)).collect()
}

/// `log2(equivalent bpp / actual bpp)` for one score.
fn gain(baseline: &Baseline, score: &Score, bpp: f64) -> Option<f64> {
    let eq = baseline.equivalent_bpp(&score.key(), score.value, score.better)?;
    (eq > 0.0 && bpp > 0.0).then(|| (eq / bpp).log2())
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
        let mut gains = BTreeMap::new();
        if let (Some(b), Some(bpp)) = (self.baseline, bpp) {
            for s in &scores {
                if let Some(g) = gain(b, s, bpp) {
                    gains.insert(s.key(), g);
                }
            }
        }
        let disagreement = gains.get(self.target).and_then(|t| {
            let others: Vec<f64> = gains
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
            gain: gains,
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
