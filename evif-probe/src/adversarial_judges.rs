//! Pinned research-profile and SafeSyn teacher adapters. Existing judges remain available.
use crate::{Res, image::Rgb8Image, judge::Judge};
use evif::{Better, Score};
use sha2::{Digest, Sha256};
use std::sync::OnceLock;
use zensim_research::{Zensim, ZensimProfile, profile::ProfileParams};

#[cfg(all(test, feature = "adversarial-models-test", target_arch = "x86_64"))]
#[path = "adversarial_golden.rs"]
mod golden;

pub(crate) const RESEARCH: &str = "a4a00e250b89c1217a110100f16c6b1e1ef2f929";
pub(crate) const GENERAL_SHA: &str =
    "e460a214c1e0c4f48bc7e3bb4705df548f9f172807cc63f39ba17cb1d94e05a0";
pub(crate) const CODEC_SHA: &str =
    "f7ecd60299482210fe33586e8d4674fe9ba163f17f2dbaf54c6e99d5ef44af63";
static GENERAL: OnceLock<Res<&'static [u8]>> = OnceLock::new();
static CODEC: OnceLock<Res<&'static [u8]>> = OnceLock::new();

pub(crate) fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn load(codec: bool) -> Res<&'static [u8]> {
    let (cell, env, expected) = if codec {
        (&CODEC, "EVIF_ECODEC_MODEL", CODEC_SHA)
    } else {
        (&GENERAL, "EVIF_E_MODEL", GENERAL_SHA)
    };
    cell.get_or_init(|| {
        let path = std::env::var(env).map_err(|_| format!("{env} must name the pinned model"))?;
        let bytes = std::fs::read(&path).map_err(|e| format!("{path}: {e}"))?;
        if sha(&bytes) != expected {
            return Err(format!("{env}: expected model SHA256 {expected}"));
        }
        Ok(&*Box::leak(bytes.into_boxed_slice()))
    })
    .clone()
}
fn general_bytes() -> &'static [u8] {
    GENERAL.get().unwrap().as_ref().unwrap()
}
fn codec_bytes() -> &'static [u8] {
    CODEC.get().unwrap().as_ref().unwrap()
}

pub(crate) struct Research {
    codec: bool,
    metric: Res<Zensim>,
}
impl Research {
    pub(crate) fn new(codec: bool) -> Self {
        static GENERAL_PARAMS: OnceLock<ProfileParams> = OnceLock::new();
        static CODEC_PARAMS: OnceLock<ProfileParams> = OnceLock::new();
        let metric = load(codec).map(|_| {
            let cell = if codec {
                &CODEC_PARAMS
            } else {
                &GENERAL_PARAMS
            };
            let params = cell.get_or_init(|| {
                ProfileParams::builder()
                    .skip_score_mapping(true)
                    .mlp(if codec { codec_bytes } else { general_bytes })
                    .extended_features(true)
                    .compute_iw_features(true)
                    .extrapolate_score(true)
                    .build()
            });
            Zensim::new(ZensimProfile::Custom {
                params,
                name: if codec { "rev7-codec" } else { "rev7-general" },
            })
        });
        Self { codec, metric }
    }
}
impl Judge for Research {
    fn name(&self) -> &'static str {
        if self.codec {
            "zensim-e-codec"
        } else {
            "zensim-e"
        }
    }
    fn keys(&self) -> Vec<String> {
        vec![self.name().into()]
    }
    fn score(&self, r: &Rgb8Image, d: &Rgb8Image) -> Res<Vec<Score>> {
        let metric = self.metric.as_ref().map_err(Clone::clone)?;
        let r = zensim_research::RgbSlice::new(r.triples(), r.width as usize, r.height as usize);
        let d = zensim_research::RgbSlice::new(d.triples(), d.width as usize, d.height as usize);
        let value = metric
            .compute(&r, &d)
            .map_err(|e| format!("research zensim: {e}"))?
            .score();
        let hash = if self.codec { CODEC_SHA } else { GENERAL_SHA };
        Ok(vec![
            Score::new(self.name(), value, Better::Higher)
                .version("0.3.0")
                .implementation(format!(
                    "zensimx@{RESEARCH};profile={};bake-sha256={hash}",
                    self.name()
                )),
        ])
    }
}

pub(crate) struct Frozen;
impl Judge for Frozen {
    fn name(&self) -> &'static str {
        "ssim2-frozen"
    }
    fn keys(&self) -> Vec<String> {
        vec![self.name().into()]
    }
    fn score(&self, r: &Rgb8Image, d: &Rgb8Image) -> Res<Vec<Score>> {
        let value = ssim2_frozen::compute_ssimulacra2(
            imgref::Img::new(r.triples(), r.width as usize, r.height as usize),
            imgref::Img::new(d.triples(), d.width as usize, d.height as usize),
        )
        .map_err(|e| format!("frozen teacher: {e}"))?;
        Ok(vec![
            Score::new(self.name(), value, Better::Higher)
                .version("0.8.2")
                .implementation("fast-ssim2@b7c2b4b33f3a26b753caabd5264a7fef9c4f4545;crate=0.8.2"),
        ])
    }
}

/// Keep the native map that supplied both scalar reductions, keyed by pixels and metric pin.
pub(crate) struct MappedButteraugli(pub std::path::PathBuf);
impl Judge for MappedButteraugli {
    fn name(&self) -> &'static str {
        "butteraugli"
    }
    fn keys(&self) -> Vec<String> {
        vec!["butteraugli".into(), "butteraugli/3-norm".into()]
    }
    fn score(&self, r: &Rgb8Image, d: &Rgb8Image) -> Res<Vec<Score>> {
        use std::io::Write;
        let result = butteraugli::butteraugli(
            r.imgref(),
            d.imgref(),
            &butteraugli::ButteraugliParams::default().with_compute_diffmap(true),
        )
        .map_err(|e| format!("butteraugli map: {e}"))?;
        let map = result.diffmap.as_ref().ok_or("butteraugli map absent")?;
        let mut key = Sha256::new();
        key.update(r.width.to_le_bytes());
        key.update(r.height.to_le_bytes());
        key.update(&r.data);
        key.update(&d.data);
        key.update(crate::pins::BUTTERAUGLI.commit.as_bytes());
        let key: String = key.finalize().iter().map(|b| format!("{b:02x}")).collect();
        let name = format!("{key}.pfm");
        let mut bytes = format!("Pf\n{} {}\n-1.0\n", r.width, r.height).into_bytes();
        // PFM stores bottom-to-top rows. The native map is contiguous.
        for row in map.buf().chunks_exact(r.width as usize).rev() {
            for value in row {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        let path = self.0.join(&name);
        match std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)
        {
            Ok(mut f) => f.write_all(&bytes).map_err(|e| e.to_string())?,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                if std::fs::read(&path).map_err(|e| e.to_string())? != bytes {
                    return Err("map CAS collision".into());
                }
            }
            Err(e) => return Err(e.to_string()),
        }
        let tag = |mut s: Score| {
            s.extra.insert("diffmap".into(), serde_json::json!({"path":format!("maps/{name}"),
                "sha256":sha(&bytes),"reference_pixels_sha256":sha(&r.data),"distorted_pixels_sha256":sha(&d.data),
                "format":"PFM grayscale f32 little endian bottom-to-top","width":r.width,"height":r.height}));
            s.version(crate::pins::BUTTERAUGLI.version)
                .implementation(crate::pins::BUTTERAUGLI.implementation("butteraugli"))
        };
        Ok(vec![
            tag(Score::new("butteraugli", result.score, Better::Lower)),
            tag(Score::new("butteraugli", result.pnorm_3, Better::Lower).variant("3-norm")),
        ])
    }
}
