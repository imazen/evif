//! JPEG AI possession. The model directory is supplied by the caller's environment.
use super::{Encoded, Family};
use crate::{Res, image::Rgb8Image, pins, rng::Rng};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    io::Read,
    path::{Path, PathBuf},
};

const IMPLEMENTATION: &str = "zenjpegai@1192fca6b6c2fdc4dcf29a6c8064c53a987caf59";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    implementation: String,
    model_id: u8,
    /// SHA-256 of the six checkpoint files this model and BOP encode use.
    model_sha256: String,
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
            model_sha256: model_dir()
                .and_then(|dir| model_fingerprint(&dir, 1))
                .unwrap_or_default(),
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
        if self.model_sha256.len() != 64
            || !self
                .model_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err("JPEG AI recipe needs a lowercase model SHA-256 digest".into());
        }
        Ok(())
    }
}

fn model_dir() -> Res<PathBuf> {
    std::env::var_os("ZENJPEGAI_MODELS")
        .map(PathBuf::from)
        .ok_or("set ZENJPEGAI_MODELS to the reference checkpoint directory".into())
}

/// Bind the exact checkpoints loaded by the pinned BOP encoder and decoder.
fn model_fingerprint(root: &Path, model_id: u8) -> Res<String> {
    let beta = ["0.002", "0.012", "0.075", "0.5"]
        .get(model_id as usize)
        .ok_or("JPEG AI model ID outside supported range")?;
    let paths = [
        format!("VM_common_int/Y_{beta}.pth"),
        format!("VM_common_int/UV_{beta}.pth"),
        format!("VM_bop/encoder_Y_{beta}.pth"),
        format!("VM_bop/encoder_UV_{beta}.pth"),
        format!("VM_bop/decoder_Y_{beta}.pth"),
        format!("VM_bop/decoder_UV_{beta}.pth"),
    ];
    let mut hash = Sha256::new();
    hash.update(b"EVIF zenjpegai BOP checkpoints v1\0");
    for rel in paths {
        let path = root.join(&rel);
        let mut file = std::fs::File::open(&path)
            .map_err(|e| format!("JPEG AI checkpoint {}: {e}", path.display()))?;
        let len = file.metadata().map_err(|e| e.to_string())?.len();
        hash.update((rel.len() as u64).to_le_bytes());
        hash.update(rel.as_bytes());
        hash.update(len.to_le_bytes());
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let n = file.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
    }
    let mut digest = String::with_capacity(64);
    use std::fmt::Write as _;
    for byte in hash.finalize() {
        write!(&mut digest, "{byte:02x}").expect("writing to String");
    }
    Ok(digest)
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
        p.model_sha256 = model_dir()
            .and_then(|dir| model_fingerprint(&dir, p.model_id))
            .unwrap_or_default();
        serde_json::to_value(p).unwrap()
    }
    fn encode(&self, params: &Value, reference: &Rgb8Image) -> Res<Encoded> {
        let p: Params =
            serde_json::from_value(params.clone()).map_err(|e| format!("params: {e}"))?;
        let models = model_dir()?;
        p.validate()?;
        if p.model_sha256 != model_fingerprint(&models, p.model_id)? {
            return Err("JPEG AI checkpoint bytes differ from the recorded recipe".into());
        }
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
        if p.model_sha256 != model_fingerprint(&models, p.model_id)? {
            return Err("JPEG AI checkpoint bytes changed during encode/decode".into());
        }
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
