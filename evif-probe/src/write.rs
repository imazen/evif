//! Assemble an EVIF file from a scored encode.

use evif::{Evif, Recipe, Scores, Source};

use crate::Res;
use crate::family::{Encoded, Family};
use crate::image::Rgb8Image;
use crate::search::Sample;

pub fn build(
    family: &dyn Family,
    reference: &Rgb8Image,
    source_id: &str,
    target: Option<&str>,
    sample: &Sample,
    enc: &Encoded,
) -> Res<Evif> {
    let png = enc.pixels.to_png()?;
    let mut file = Evif::new(enc.pixels.header(), png, enc.pixels.digest());
    file.digests.reference = Some(reference.digest());
    file.bits = enc.bits.clone();
    file.claimed_bytes = if enc.bits.is_none() {
        enc.claimed_bytes
    } else {
        None
    };
    let mut source = Source::default();
    source.id = Some(source_id.to_string());
    file.source = Some(source);

    let mut recipe = Recipe::new(family.name(), sample.params.clone());
    if let Some((codec, pin)) = family.codec() {
        recipe.codec = Some(codec.to_string());
        recipe.codec_version = Some(pin.version.to_string());
        recipe.codec_commit = Some(pin.commit.to_string());
    }
    recipe.target = target.map(str::to_string);
    file.recipe = Some(recipe);

    let mut scores = Scores::new(sample.scores.clone());
    if !sample.gain.is_empty() {
        scores.extra.insert(
            "gain_vs_baseline".into(),
            serde_json::to_value(&sample.gain).map_err(|e| e.to_string())?,
        );
    }
    if let Some(d) = sample.disagreement {
        scores.extra.insert("disagreement".into(), d.into());
    }
    file.scores = Some(scores);
    Ok(file)
}
