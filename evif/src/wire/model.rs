use alloc::{string::String, vec::Vec};
use core::fmt;
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

/// Algorithm-tagged digest. Unknown algorithms and larger digests are preserved.
/// JSON uses an explicit algorithm ID and a nonempty, even-length lowercase hex
/// digest. Resource limits apply to the enclosing JSON, not a fixed hash width.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "HashFields")]
pub struct Hash {
    algorithm: String,
    digest: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HashFields {
    algorithm: String,
    digest: String,
}
impl TryFrom<HashFields> for Hash {
    type Error = Error;
    fn try_from(fields: HashFields) -> Result<Self, Error> {
        let hash = Self {
            algorithm: fields.algorithm,
            digest: fields.digest,
        };
        hash.validate()?;
        Ok(hash)
    }
}
impl Hash {
    /// Construct a validated record; unknown algorithm IDs are permitted.
    pub fn new(algorithm: &str, digest: &str) -> Result<Self, Error> {
        HashFields {
            algorithm: algorithm.into(),
            digest: digest.into(),
        }
        .try_into()
    }
    /// Hash exact object bytes with SHA-256 (no domain prefix).
    pub fn of(bytes: &[u8]) -> Self {
        Self::sha256(Sha256::digest(bytes).into())
    }
    /// Stable algorithm identifier. Unknown does not mean verified.
    pub fn algorithm(&self) -> &str {
        &self.algorithm
    }
    /// Canonical lowercase hexadecimal digest, in algorithm-defined byte order.
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub(crate) fn sha256(bytes: [u8; 32]) -> Self {
        let mut digest = String::with_capacity(64);
        use core::fmt::Write;
        for byte in bytes {
            write!(digest, "{byte:02x}").expect("String write");
        }
        Self {
            algorithm: "sha256".into(),
            digest,
        }
    }
    fn validate(&self) -> Result<(), Error> {
        let id = self.algorithm.as_bytes();
        check(
            !id.is_empty()
                && id.len() <= 64
                && id[0].is_ascii_lowercase()
                && id
                    .iter()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-'),
            "invalid hash algorithm",
        )?;
        check(
            !self.digest.is_empty()
                && self.digest.len().is_multiple_of(2)
                && self
                    .digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid hash digest",
        )?;
        let width = match self.algorithm.as_str() {
            "xxh3-128" => Some(32),
            "sha256" => Some(64),
            _ => None,
        };
        check(
            width.is_none_or(|n| self.digest.len() == n),
            "hash algorithm/digest length mismatch",
        )
    }
    pub(crate) fn require_sha256(&self) -> Result<(), Error> {
        if self.algorithm != "sha256" {
            return Err(Error::Unsupported(
                "hash algorithm for payload/media verification",
            ));
        }
        Ok(())
    }
    fn bind(&self, h: &mut Sha256) {
        h.update((self.algorithm.len() as u64).to_le_bytes());
        h.update(self.algorithm.as_bytes());
        h.update((self.digest.len() as u64 / 2).to_le_bytes());
        // The digest is validated ASCII hex. Bind its bytes, not its spelling.
        for pair in self.digest.as_bytes().as_chunks::<2>().0 {
            let nibble = |b: u8| if b <= b'9' { b - b'0' } else { b - b'a' + 10 };
            h.update([(nibble(pair[0]) << 4) | nibble(pair[1])]);
        }
    }
}
impl fmt::Display for Hash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.algorithm, self.digest)
    }
}

/// Canonical sample identity plus optional independent checks over the same bytes.
/// Writers default to XXH3-128. Other algorithms are preserved without asserting
/// that decoded samples were verified. Checks never contribute to media identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SampleHashFields")]
pub struct SampleHashes {
    pub canonical: Hash,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<Hash>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SampleHashFields {
    canonical: Hash,
    #[serde(default)]
    checks: Vec<Hash>,
}
impl TryFrom<SampleHashFields> for SampleHashes {
    type Error = Error;
    fn try_from(fields: SampleHashFields) -> Result<Self, Error> {
        let mut algorithms = alloc::collections::BTreeSet::new();
        algorithms.insert(fields.canonical.algorithm());
        for hash in &fields.checks {
            check(
                algorithms.insert(hash.algorithm()),
                "duplicate sample hash algorithm",
            )?;
        }
        Ok(Self {
            canonical: fields.canonical,
            checks: fields.checks,
        })
    }
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
        h.update(b"EVIF/media/4\0");
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
        canonical: Hash::new("xxh3-128", &alloc::format!("{:032x}", xxh3.digest128()))?,
        checks: sha256
            .into_iter()
            .map(|h| Hash::sha256(h.finalize().into()))
            .collect(),
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
        self.samples.canonical.bind(h);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
#[non_exhaustive]
pub enum Storage {
    /// Immediately followed by one PNGF or IMIJ record.
    Embedded { hash: Option<Hash> },
    /// A self-contained IMI page bundle. Select consecutive pages starting at
    /// `first_page`; other pages are codec prediction dependencies, not frames.
    EmbeddedPages {
        hash: Option<Hash>,
        first_page: U64,
        total_pages: U64,
    },
    /// Reuse an earlier EmbeddedPages payload, selecting another page window.
    /// References are direct, backward-only, and do not create dependency chains.
    SharedPages {
        source_asset: u32,
        source_first_frame: U64,
        first_page: U64,
    },
    /// Never fetched automatically. Digest covers the exact external bytes.
    External { uri: String, hash: Hash },
}

/// One frame window in a self-contained carrier, optionally shared. Subrectangles, posters and codec
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
    pub hash: Hash,
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
