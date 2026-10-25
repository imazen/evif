use alloc::{string::String, vec::Vec};
use core::{fmt, str::FromStr};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use sha2::{Digest, Sha256};
use zenhash::Xxh3;

use super::{Error, check};

/// A full-width unsigned integer, encoded as a canonical decimal JSON string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct U64(pub u64);

impl Serialize for U64 {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&self.0)
    }
}
impl<'de> Deserialize<'de> for U64 {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        if text.is_empty()
            || (text.len() > 1 && text.starts_with('0'))
            || !text.bytes().all(|b| b.is_ascii_digit())
        {
            return Err(de::Error::custom("expected canonical u64 string"));
        }
        text.parse().map(Self).map_err(de::Error::custom)
    }
}

/// SHA-256, encoded as 64 lowercase hexadecimal characters in JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Hash(pub [u8; 32]);
impl Hash {
    /// Hash exact object bytes (no domain prefix).
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }
}
impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}
impl FromStr for Hash {
    type Err = Error;
    fn from_str(text: &str) -> Result<Self, Error> {
        check(
            text.len() == 64
                && text
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid SHA-256",
        )?;
        let mut bytes = [0; 32];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16)
                .map_err(|_| Error::Invalid("invalid SHA-256"))?;
        }
        Ok(Self(bytes))
    }
}
impl Serialize for Hash {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}
impl<'de> Deserialize<'de> for Hash {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?.parse().map_err(de::Error::custom)
    }
}

/// Canonical XXH3-128 sample digest, serialized as 32 lowercase hex digits.
/// Uses the default (zero) seed and standard XXH3 secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SampleHash(pub u128);
impl fmt::Display for SampleHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:032x}", self.0)
    }
}
impl FromStr for SampleHash {
    type Err = Error;
    fn from_str(text: &str) -> Result<Self, Error> {
        check(
            text.len() == 32
                && text
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid XXH3-128",
        )?;
        u128::from_str_radix(text, 16)
            .map(Self)
            .map_err(|_| Error::Invalid("invalid XXH3-128"))
    }
}
impl Serialize for SampleHash {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}
impl<'de> Deserialize<'de> for SampleHash {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        String::deserialize(d)?.parse().map_err(de::Error::custom)
    }
}

/// Required canonical sample identity plus an optional independent check.
/// Both algorithms consume the same frame descriptor and packed sample bytes.
/// SHA-256 presence or value does not contribute to media identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SampleHashes {
    #[serde(rename = "xxh3-128")]
    pub xxh3_128: SampleHash,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<Hash>,
}

/// Positive reduced rational; units are defined by the containing field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rational {
    pub num: U64,
    pub den: U64,
}
impl Rational {
    pub fn new(num: u64, den: u64) -> Result<Self, Error> {
        let r = Self {
            num: U64(num),
            den: U64(den),
        };
        r.validate()?;
        Ok(r)
    }
    pub(crate) fn validate(self) -> Result<(), Error> {
        let (mut a, mut b) = (self.num.0, self.den.0);
        check(a > 0 && b > 0, "rational must be positive")?;
        while b != 0 {
            (a, b) = (b, a % b);
        }
        check(a == 1, "rational must be reduced")
    }
    fn hash(self, h: &mut Sha256) {
        h.update(self.num.0.to_le_bytes());
        h.update(self.den.0.to_le_bytes());
    }
}

macro_rules! vocabulary {
    ($name:ident { $($v:ident = $n:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(rename_all = "kebab-case")]
        #[repr(u8)]
        #[non_exhaustive]
        pub enum $name { $($v = $n),+ }
    }
}
vocabulary!(Kind { Still = 1, Sequence = 2 });
vocabulary!(Channels { Gray = 1, GrayAlpha = 2, Rgb = 3, Rgba = 4 });
vocabulary!(Samples { U8 = 1, U16 = 2, F16 = 3, F32 = 4 });
vocabulary!(Primaries { Bt709 = 1, DisplayP3D65 = 2, Bt2020 = 3 });
vocabulary!(Transfer { Srgb = 1, Bt709 = 2, Pq = 3, Hlg = 4, Linear = 5 });
vocabulary!(Domain { DisplayRelative = 1, SceneRelative = 2, DisplayAbsolute = 3 });
vocabulary!(Carrier { Png = 1, Apng = 2, Imi = 3 });
impl Samples {
    pub fn bytes(self) -> usize {
        match self {
            Self::U8 => 1,
            Self::U16 | Self::F16 => 2,
            Self::F32 => 4,
        }
    }
}
impl Carrier {
    pub fn tag(self) -> [u8; 4] {
        match self {
            Self::Png | Self::Apng => *b"PNGF",
            Self::Imi => *b"IMIJ",
        }
    }
}

/// Resolved complete-frame RGB/gray profile. All alpha is straight; all integer
/// code values are full-range. Orientation is already applied, scan progressive.
/// IDs are file-local addresses, not semantic identity or reference/candidate roles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub id: u32,
    /// Must be `resolved-rgb/1`. New interpretation requires a new profile.
    pub profile: String,
    pub kind: Kind,
    pub width: u32,
    pub height: u32,
    pub channels: Channels,
    pub samples: Samples,
    pub primaries: Primaries,
    pub transfer: Transfer,
    pub domain: Domain,
    /// Required only for absolute linear float: sample * scale = nits.
    pub nits_per_unit: Option<Rational>,
    pub pixel_aspect: Rational,
    pub frame_count: U64,
    /// None for stills. Sequence: total plays; zero means infinite looping.
    pub plays: Option<U64>,
}
impl Asset {
    pub fn validate(&self) -> Result<(), Error> {
        check(self.id != 0, "asset ID zero is reserved")?;
        if self.profile != "resolved-rgb/1" {
            return Err(Error::Unsupported("media profile"));
        }
        check(
            self.width > 0 && self.height > 0 && self.frame_count.0 > 0,
            "empty media",
        )?;
        self.pixel_aspect.validate()?;
        match self.kind {
            Kind::Still => check(
                self.frame_count.0 == 1 && self.plays.is_none(),
                "still timing",
            )?,
            Kind::Sequence => check(self.plays.is_some(), "sequence play count missing")?,
        }
        let domain_ok = match self.transfer {
            Transfer::Srgb => self.domain == Domain::DisplayRelative,
            Transfer::Bt709 | Transfer::Hlg => self.domain == Domain::SceneRelative,
            Transfer::Pq => self.domain == Domain::DisplayAbsolute,
            Transfer::Linear => true,
        };
        check(domain_ok, "transfer/domain mismatch")?;
        if self.transfer == Transfer::Linear && self.domain == Domain::DisplayAbsolute {
            check(
                matches!(self.samples, Samples::F16 | Samples::F32),
                "absolute linear requires float",
            )?;
            self.nits_per_unit
                .ok_or(Error::Invalid("missing nit scale"))?
                .validate()?;
        } else {
            check(self.nits_per_unit.is_none(), "unexpected nit scale")?;
        }
        Ok(())
    }
    pub(crate) fn hasher(&self) -> Sha256 {
        let mut h = Sha256::new();
        h.update(b"EVIF/media/3\0");
        h.update([1, self.kind as u8]); // resolved-rgb profile revision
        h.update(self.width.to_le_bytes());
        h.update(self.height.to_le_bytes());
        h.update([
            self.channels as u8,
            self.samples as u8,
            self.primaries as u8,
            self.transfer as u8,
            self.domain as u8,
        ]);
        h.update([u8::from(self.nits_per_unit.is_some())]);
        if let Some(scale) = self.nits_per_unit {
            scale.hash(&mut h);
        }
        self.pixel_aspect.hash(&mut h);
        h.update(self.frame_count.0.to_le_bytes());
        h.update([u8::from(self.plays.is_some())]);
        if let Some(plays) = self.plays {
            h.update(plays.0.to_le_bytes());
        }
        h
    }
}

/// Hash a decoded frame, skipping row padding. Multibyte samples must already
/// be little-endian, including raw IEEE float bits. No color conversion or
/// nonfinite-value repair is performed. Returns canonical XXH3-128 and additional
/// SHA-256, computed in one traversal. See [`frame_hash_xxh3`] for XXH3 alone.
pub fn frame_hash(
    asset: &Asset,
    samples_le: &[u8],
    stride_bytes: usize,
) -> Result<SampleHashes, Error> {
    hash_frame(asset, samples_le, stride_bytes, true)
}

/// Hash the same canonical frame bytes with XXH3-128 only, without computing
/// SHA-256. The result has the same canonical identity as [`frame_hash`].
pub fn frame_hash_xxh3(
    asset: &Asset,
    samples_le: &[u8],
    stride_bytes: usize,
) -> Result<SampleHashes, Error> {
    hash_frame(asset, samples_le, stride_bytes, false)
}

fn hash_frame(
    asset: &Asset,
    samples_le: &[u8],
    stride_bytes: usize,
    with_sha256: bool,
) -> Result<SampleHashes, Error> {
    asset.validate()?;
    let row = usize::try_from(asset.width)
        .ok()
        .and_then(|w| w.checked_mul(asset.channels as usize))
        .and_then(|n| n.checked_mul(asset.samples.bytes()))
        .ok_or(Error::Limit("frame row"))?;
    let height = usize::try_from(asset.height).map_err(|_| Error::Limit("frame height"))?;
    let need = (height - 1)
        .checked_mul(stride_bytes)
        .and_then(|n| n.checked_add(row))
        .ok_or(Error::Limit("frame size"))?;
    check(
        stride_bytes >= row && samples_le.len() >= need,
        "frame buffer/stride mismatch",
    )?;
    let mut xxh3 = Xxh3::new();
    let mut sha256 = with_sha256.then(Sha256::new);
    let mut update = |bytes: &[u8]| {
        xxh3.update(bytes);
        if let Some(h) = &mut sha256 {
            h.update(bytes);
        }
    };
    update(b"EVIF/frame/1\0");
    update(&asset.width.to_le_bytes());
    update(&asset.height.to_le_bytes());
    update(&[asset.channels as u8, asset.samples as u8]);
    for y in 0..height {
        update(&samples_le[y * stride_bytes..y * stride_bytes + row]);
    }
    Ok(SampleHashes {
        xxh3_128: SampleHash(xxh3.digest128()),
        sha256: sha256.map(|h| Hash(h.finalize().into())),
    })
}

/// Frames are ordered. PTS is the exact sum of preceding durations, beginning
/// at zero; no redundant FPS or rounded timestamp is stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub duration: Option<Rational>,
    pub samples: SampleHashes,
}
impl Frame {
    pub(crate) fn hash(&self, h: &mut Sha256) {
        h.update([u8::from(self.duration.is_some())]);
        if let Some(time) = self.duration {
            time.hash(h);
        }
        h.update(self.samples.xxh3_128.0.to_le_bytes());
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
#[non_exhaustive]
pub enum Storage {
    /// Immediately followed by one PNGF or IMIJ record.
    Embedded { sha256: Option<Hash> },
    /// Never fetched automatically. Digest covers the exact external bytes.
    External { uri: String, sha256: Hash },
}

/// One independently decodable segment. Subrectangles, posters and codec
/// preroll do not count as logical presentation frames.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Segment {
    pub asset: u32,
    pub first_frame: U64,
    pub carrier: Carrier,
    pub frames: Vec<Frame>,
    pub storage: Storage,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaRoot {
    pub asset: u32,
    pub sha256: Hash,
}

/// Container-defined envelope; `data` is an adapter-owned JSON object. The
/// reference hash may address external media. Candidate must be in this file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScoreRecord {
    pub schema: u32,
    pub id: String,
    pub supersedes: Option<String>,
    pub candidate: Hash,
    pub reference: Option<Hash>,
    pub adapter: String,
    pub data: serde_json::Value,
}
