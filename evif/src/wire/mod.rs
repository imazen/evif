//! Flat wire-major-1 records. See `docs/wire-format-1.md` for the complete grammar.
//! Parsing verifies framing, media descriptions, segment coverage, declared payload
//! hashes and media roots. It does NOT decode carriers or certify sample hashes.
//! Legacy major-0 files remain available through [`crate::Evif`].

mod json;
mod model;
#[cfg(feature = "std")]
mod stream;
pub use model::*;
#[cfg(feature = "std")]
pub use stream::{Writer, append_record, append_score, read};

use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
    vec::Vec,
};
use core::fmt;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};

/// Signature, major/minor u16, reserved u32. All binary integers are LE.
pub const HEADER: [u8; 16] = [
    0x8b, b'E', b'V', b'I', b'F', 13, 10, 26, 1, 0, 0, 0, 0, 0, 0, 0,
];

#[derive(Debug)]
#[non_exhaustive]
pub enum Error {
    Invalid(&'static str),
    Unsupported(&'static str),
    Limit(&'static str),
    Json(serde_json::Error),
    #[cfg(feature = "std")]
    Io(std::io::Error),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(s) => write!(f, "invalid EVIF: {s}"),
            Self::Unsupported(s) => write!(f, "unsupported EVIF: {s}"),
            Self::Limit(s) => write!(f, "EVIF resource limit: {s}"),
            Self::Json(e) => write!(f, "EVIF JSON: {e}"),
            #[cfg(feature = "std")]
            Self::Io(e) => write!(f, "EVIF I/O: {e}"),
        }
    }
}
impl core::error::Error for Error {}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
#[cfg(feature = "std")]
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
fn check(ok: bool, reason: &'static str) -> Result<(), Error> {
    if ok {
        Ok(())
    } else {
        Err(Error::Invalid(reason))
    }
}

/// Caller-selected resource bounds, not wire-format maxima. JSON remains bounded
/// even when compressed payloads exceed 4 GiB. Increasing limits is explicit.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_assets: usize,
    pub max_records: u64,
    pub max_json_bytes: usize,
    pub max_total_json_bytes: u64,
    pub max_frames_per_segment: usize,
    pub max_total_frames: u64,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_assets: 1024,
            max_records: 1_000_000,
            max_json_bytes: 4 * 1024 * 1024,
            max_total_json_bytes: 64 * 1024 * 1024,
            max_frames_per_segment: 65536,
            max_total_frames: 1_000_000,
        }
    }
}

/// Absolute payload location; no codec bytes are copied into the index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub offset: u64,
    pub length: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedSegment {
    pub segment: Segment,
    pub payload: Option<Span>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedBits {
    pub asset: u32,
    pub codec: [u8; 4],
    pub payload: Span,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ancillary {
    pub tag: [u8; 4],
    pub payload: Span,
}

/// Bounded metadata index. Offsets can be used with seek/range reads; assets and
/// segments retain no compressed payload bytes. Unknown ancillary bytes are
/// preserved by their spans, not discarded or interpreted.
#[derive(Debug)]
pub struct Index {
    pub assets: BTreeMap<u32, Asset>,
    pub segments: Vec<IndexedSegment>,
    pub bitstreams: Vec<IndexedBits>,
    pub roots: Vec<MediaRoot>,
    pub scores: Vec<ScoreRecord>,
    /// Original JSON bytes for exact pass-through of scoring metadata.
    pub score_spans: Vec<Span>,
    pub ancillary: Vec<Ancillary>,
    /// End of MEND: first legal position for an appended score.
    pub media_end: u64,
    pub file_end: u64,
}

struct Progress {
    frames: u64,
    hasher: Sha256,
    bits: bool,
}
struct State {
    index: Index,
    progress: BTreeMap<u32, Progress>,
    pending: Option<Segment>,
    score_ids: BTreeSet<String>,
    limits: Limits,
    records: u64,
    json_bytes: u64,
    frames: u64,
    ended: bool,
}
impl State {
    fn new(limits: Limits) -> Self {
        Self {
            index: Index {
                assets: BTreeMap::new(),
                segments: Vec::new(),
                bitstreams: Vec::new(),
                roots: Vec::new(),
                scores: Vec::new(),
                score_spans: Vec::new(),
                ancillary: Vec::new(),
                media_end: 0,
                file_end: 16,
            },
            progress: BTreeMap::new(),
            pending: None,
            score_ids: BTreeSet::new(),
            limits,
            records: 0,
            json_bytes: 0,
            frames: 0,
            ended: false,
        }
    }
    fn start(&mut self, tag: [u8; 4], length: u64, offset: u64) -> Result<(), Error> {
        check(
            tag.iter().all(u8::is_ascii_alphabetic),
            "record tag is not ASCII letters",
        )?;
        self.records = self
            .records
            .checked_add(1)
            .ok_or(Error::Limit("record count"))?;
        if self.records > self.limits.max_records {
            return Err(Error::Limit("record count"));
        }
        self.index.file_end = offset
            .checked_add(length)
            .ok_or(Error::Invalid("record offset overflow"))?;
        if let Some(segment) = &self.pending {
            check(
                tag == segment.carrier.tag(),
                "SEGM must be followed immediately by its payload",
            )?;
        } else {
            check(tag != *b"PNGF" && tag != *b"IMIJ", "payload without SEGM")?;
        }
        if matches!(
            &tag,
            b"ASST" | b"SEGM" | b"PNGF" | b"IMIJ" | b"BITS" | b"MEND"
        ) {
            check(!self.ended, "media record after MEND")?;
        } else if tag == *b"scor" {
            check(self.ended, "score before MEND")?;
        } else if tag[0].is_ascii_uppercase() {
            return Err(Error::Unsupported("critical record"));
        }
        if is_json(tag) {
            self.json_bytes = self
                .json_bytes
                .checked_add(length)
                .ok_or(Error::Limit("JSON bytes"))?;
            if length > self.limits.max_json_bytes as u64
                || self.json_bytes > self.limits.max_total_json_bytes
            {
                return Err(Error::Limit("JSON bytes"));
            }
        }
        Ok(())
    }
    fn json(&mut self, tag: [u8; 4], body: &[u8]) -> Result<(), Error> {
        match &tag {
            b"ASST" => {
                let asset: Asset = decode_json(body)?;
                asset.validate()?;
                check(!self.progress.contains_key(&asset.id), "duplicate asset ID")?;
                if self.progress.len() >= self.limits.max_assets {
                    return Err(Error::Limit("asset count"));
                }
                self.progress.insert(
                    asset.id,
                    Progress {
                        frames: 0,
                        hasher: asset.hasher(),
                        bits: false,
                    },
                );
                self.index.assets.insert(asset.id, asset);
            }
            b"SEGM" => {
                let segment: Segment = decode_json(body)?;
                let asset = self
                    .index
                    .assets
                    .get(&segment.asset)
                    .ok_or(Error::Invalid("undeclared asset"))?;
                let progress = &self.progress[&segment.asset];
                check(
                    segment.first_frame.0 == progress.frames,
                    "segment gap, overlap or reorder",
                )?;
                check(!segment.frames.is_empty(), "empty segment")?;
                if segment.frames.len() > self.limits.max_frames_per_segment {
                    return Err(Error::Limit("frames per segment"));
                }
                let count = segment.frames.len() as u64;
                check(
                    count <= asset.frame_count.0 - progress.frames,
                    "too many frames",
                )?;
                self.frames = self
                    .frames
                    .checked_add(count)
                    .ok_or(Error::Limit("total frames"))?;
                if self.frames > self.limits.max_total_frames {
                    return Err(Error::Limit("total frames"));
                }
                check(
                    segment.carrier != Carrier::Png || count == 1,
                    "PNG must hold one frame",
                )?;
                check(
                    segment.carrier == Carrier::Imi
                        || matches!(asset.samples, Samples::U8 | Samples::U16),
                    "PNG cannot preserve float samples",
                )?;
                for frame in &segment.frames {
                    match asset.kind {
                        Kind::Still => check(frame.duration.is_none(), "still has a duration")?,
                        Kind::Sequence => {
                            let time = frame
                                .duration
                                .ok_or(Error::Invalid("sequence duration missing"))?;
                            time.validate()?;
                            if segment.carrier == Carrier::Apng {
                                check(
                                    time.num.0 <= 65535 && time.den.0 <= 65535,
                                    "APNG duration is not representable",
                                )?;
                            }
                        }
                    }
                }
                match &segment.storage {
                    Storage::Embedded { hash } => {
                        if let Some(hash) = hash {
                            hash.require_sha256()?;
                        }
                        self.pending = Some(segment);
                    }
                    Storage::EmbeddedPages {
                        hash,
                        first_page,
                        total_pages,
                    } => {
                        check(segment.carrier == Carrier::Imi, "page windows require IMI")?;
                        check(
                            total_pages.0 <= self.limits.max_frames_per_segment as u64,
                            "IMI page count exceeds configured limit",
                        )?;
                        check(
                            first_page
                                .0
                                .checked_add(count)
                                .is_some_and(|end| end <= total_pages.0),
                            "IMI page window outside carrier",
                        )?;
                        if let Some(hash) = hash {
                            hash.require_sha256()?;
                        }
                        self.pending = Some(segment);
                    }
                    Storage::SharedPages {
                        source_asset,
                        source_first_frame,
                        first_page,
                    } => {
                        check(segment.carrier == Carrier::Imi, "page windows require IMI")?;
                        let source = self
                            .index
                            .segments
                            .iter()
                            .find(|s| {
                                s.segment.asset == *source_asset
                                    && s.segment.first_frame == *source_first_frame
                            })
                            .ok_or(Error::Invalid("shared carrier must already exist"))?;
                        let Storage::EmbeddedPages { total_pages, .. } = source.segment.storage
                        else {
                            return Err(Error::Invalid(
                                "shared carrier must name an embedded page owner",
                            ));
                        };
                        check(
                            first_page
                                .0
                                .checked_add(count)
                                .is_some_and(|end| end <= total_pages.0),
                            "IMI page window outside carrier",
                        )?;
                        let mut owner = self.index.assets[source_asset].clone();
                        owner.id = asset.id;
                        owner.kind = asset.kind;
                        owner.frame_count = asset.frame_count;
                        owner.plays = asset.plays;
                        check(owner == *asset, "shared carrier signal differs")?;
                        let payload = source
                            .payload
                            .ok_or(Error::Invalid("shared carrier has no payload"))?;
                        self.commit_segment(segment, Some(payload));
                    }
                    Storage::External { uri, .. } => {
                        check(!uri.is_empty(), "empty external URI")?;
                        self.commit_segment(segment, None);
                    }
                }
            }
            b"MEND" => {
                let roots: Vec<MediaRoot> = decode_json(body)?;
                for root in &roots {
                    root.hash.require_sha256()?;
                }
                check(roots == self.roots()?, "MEND media roots differ")?;
                self.index.roots = roots;
                self.ended = true;
                self.index.media_end = self.index.file_end;
            }
            b"scor" => {
                let score: ScoreRecord = decode_json(body)?;
                if score.schema != 1 {
                    return Err(Error::Unsupported("score envelope schema"));
                }
                check(
                    !score.id.is_empty() && score.id.len() <= 256 && !score.adapter.is_empty(),
                    "score identity missing or too long",
                )?;
                check(score.data.is_object(), "score data must be a JSON object")?;
                check(
                    self.index.roots.iter().any(|r| r.hash == score.candidate),
                    "score candidate is not in media section",
                )?;
                check(
                    !self.score_ids.contains(&score.id),
                    "duplicate score record ID",
                )?;
                if let Some(prior) = &score.supersedes {
                    check(
                        self.score_ids.contains(prior),
                        "superseded score is not earlier",
                    )?;
                }
                self.score_ids.insert(score.id.clone());
                self.index.scores.push(score);
                self.index.score_spans.push(Span {
                    offset: self.index.file_end - body.len() as u64,
                    length: body.len() as u64,
                });
            }
            _ => return Err(Error::Invalid("not a JSON record")),
        }
        Ok(())
    }
    fn commit_segment(&mut self, segment: Segment, payload: Option<Span>) {
        let p = self
            .progress
            .get_mut(&segment.asset)
            .expect("validated asset");
        for frame in &segment.frames {
            frame.hash(&mut p.hasher);
        }
        p.frames += segment.frames.len() as u64;
        self.index
            .segments
            .push(IndexedSegment { segment, payload });
    }
    fn expected_payload_hash(&self) -> Option<&Hash> {
        self.pending.as_ref().and_then(|s| match &s.storage {
            Storage::Embedded { hash } | Storage::EmbeddedPages { hash, .. } => hash.as_ref(),
            _ => None,
        })
    }
    fn payload(&mut self, span: Span, actual: Option<Hash>) -> Result<(), Error> {
        check(span.length > 0, "empty rendition payload")?;
        if let Some(expected) = self.expected_payload_hash() {
            check(
                actual.as_ref() == Some(expected),
                "payload SHA-256 mismatch",
            )?;
        }
        let segment = self
            .pending
            .take()
            .ok_or(Error::Invalid("payload without segment"))?;
        self.commit_segment(segment, Some(span));
        Ok(())
    }
    fn bits(&mut self, prefix: &[u8], span: Span) -> Result<(), Error> {
        check(
            prefix.len() == 8 && span.length > 8,
            "BITS needs asset, codec and bytes",
        )?;
        let asset = u32::from_le_bytes(prefix[..4].try_into().unwrap());
        let p = self
            .progress
            .get_mut(&asset)
            .ok_or(Error::Invalid("BITS asset missing"))?;
        check(!p.bits, "duplicate BITS for asset")?;
        p.bits = true;
        self.index.bitstreams.push(IndexedBits {
            asset,
            codec: prefix[4..8].try_into().unwrap(),
            payload: Span {
                offset: span.offset + 8,
                length: span.length - 8,
            },
        });
        Ok(())
    }
    fn roots(&self) -> Result<Vec<MediaRoot>, Error> {
        check(
            !self.progress.is_empty() && self.pending.is_none(),
            "incomplete media",
        )?;
        self.progress
            .iter()
            .map(|(&id, p)| {
                check(
                    p.frames == self.index.assets[&id].frame_count.0,
                    "incomplete asset frame coverage",
                )?;
                Ok(MediaRoot {
                    asset: id,
                    hash: Hash::sha256(p.hasher.clone().finalize().into()),
                })
            })
            .collect()
    }
    fn finish(self) -> Result<Index, Error> {
        check(
            self.ended && self.pending.is_none(),
            "missing MEND or pending payload",
        )?;
        Ok(self.index)
    }
}

fn is_json(tag: [u8; 4]) -> bool {
    matches!(&tag, b"ASST" | b"SEGM" | b"MEND" | b"scor")
}
fn file_header(bytes: &[u8]) -> Result<(), Error> {
    check(
        bytes.len() == 16 && bytes[..8] == HEADER[..8],
        "file header/signature",
    )?;
    if bytes[8..10] != HEADER[8..10] {
        return Err(Error::Unsupported("wire major"));
    }
    // Minor versions may add ignorable records, not redefine existing records.
    check(bytes[12..16] == [0; 4], "reserved file flags")
}
#[cfg(feature = "std")]
fn record_header(tag: [u8; 4], len: u64) -> [u8; 12] {
    let mut h = [0; 12];
    h[..4].copy_from_slice(&tag);
    h[4..].copy_from_slice(&len.to_le_bytes());
    h
}
fn decode_json<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, Error> {
    json::decode(bytes)
}

/// Parse a complete major-1 file without copying its payloads. Strict about an
/// incomplete trailing record. For transport only, unknown ancillary records are
/// retained as spans. External objects are not fetched and samples are not decoded.
pub fn scan(bytes: &[u8], limits: Limits) -> Result<Index, Error> {
    file_header(
        bytes
            .get(..16)
            .ok_or(Error::Invalid("truncated file header"))?,
    )?;
    let mut state = State::new(limits);
    let mut pos = 16usize;
    while pos < bytes.len() {
        let header_end = pos
            .checked_add(12)
            .ok_or(Error::Invalid("record overflow"))?;
        let h = bytes
            .get(pos..header_end)
            .ok_or(Error::Invalid("truncated record header"))?;
        let tag = h[..4].try_into().unwrap();
        let len = u64::from_le_bytes(h[4..].try_into().unwrap());
        state.start(tag, len, header_end as u64)?;
        let end = usize::try_from(len)
            .ok()
            .and_then(|n| header_end.checked_add(n))
            .ok_or(Error::Invalid("record length overflow"))?;
        let body = bytes
            .get(header_end..end)
            .ok_or(Error::Invalid("truncated record body"))?;
        let span = Span {
            offset: header_end as u64,
            length: len,
        };
        if is_json(tag) {
            state.json(tag, body)?;
        } else if matches!(&tag, b"PNGF" | b"IMIJ") {
            let hash = state.expected_payload_hash().map(|_| Hash::of(body));
            state.payload(span, hash)?;
        } else if tag == *b"BITS" {
            state.bits(body.get(..8).ok_or(Error::Invalid("short BITS"))?, span)?;
        } else {
            state.index.ancillary.push(Ancillary { tag, payload: span });
        }
        pos = end;
    }
    state.finish()
}
