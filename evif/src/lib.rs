//! EVIF, the evil image format.
//!
//! An EVIF file wraps one distorted image for image-quality-metric testing:
//! the interior bitstream an encoder produced, a lossless PNG rendition of the
//! decoded pixels, the recipe that made it, and the metric scores computed on
//! that rendition. The byte layout is in `SPEC.md` at the repository root.
//!
//! This crate reads and writes the container and computes pixel digests. It
//! decodes no image codec: the rendition and bitstream are opaque bytes here.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

mod digest;
mod error;
mod meta;

use alloc::vec::Vec;

pub use digest::{pixel_digest_u8, pixel_digest_u16};
pub use error::Error;
pub use meta::{Better, Recipe, Score, Scores, Source};

/// File signature: `\x8BEVIF\r\n\x1A`.
pub const SIGNATURE: [u8; 8] = [0x8B, b'E', b'V', b'I', b'F', 0x0D, 0x0A, 0x1A];
/// Major version this crate writes and reads.
pub const MAJOR: u8 = 0;
/// Minor version this crate writes.
pub const MINOR: u8 = 0;

const HEADER_LEN: usize = 12;

/// Channel layout of the raster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Layout {
    Gray = 1,
    GrayAlpha = 2,
    Rgb = 3,
    Rgba = 4,
}

impl Layout {
    /// Channels per pixel.
    pub fn channels(self) -> usize {
        self as usize
    }

    fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            1 => Self::Gray,
            2 => Self::GrayAlpha,
            3 => Self::Rgb,
            4 => Self::Rgba,
            _ => return None,
        })
    }
}

/// Sample type of the raster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Sample {
    U8 = 1,
    U16 = 2,
}

impl Sample {
    /// Bytes per sample.
    pub fn bytes(self) -> usize {
        self as usize
    }

    fn from_u8(v: u8) -> Option<Self> {
        Some(match v {
            1 => Self::U8,
            2 => Self::U16,
            _ => return None,
        })
    }
}

/// The `HEAD` chunk: geometry and storage format of the rendition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub width: u32,
    pub height: u32,
    pub layout: Layout,
    pub sample: Sample,
}

impl Header {
    pub fn new(width: u32, height: u32, layout: Layout, sample: Sample) -> Self {
        Self {
            width,
            height,
            layout,
            sample,
        }
    }

    /// Pixel count.
    pub fn pixels(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    fn validate(&self) -> Result<(), Error> {
        if self.width == 0 || self.height == 0 || self.width > 65535 || self.height > 65535 {
            return Err(Error::Corrupt("dimensions out of range"));
        }
        if self.pixels() >= 1 << 31 {
            return Err(Error::Corrupt("too many pixels"));
        }
        Ok(())
    }
}

/// A four-byte codec or format tag, such as `*b"jpeg"` or `*b"jxl "`.
pub type FourCc = [u8; 4];

/// The `BITS` chunk: the interior bitstream, byte for byte.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bitstream {
    pub codec: FourCc,
    pub data: Vec<u8>,
}

/// The `DIGS` chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Digests {
    /// Digest of the `PIXL` raster.
    pub distorted: u64,
    /// Digest of the reference raster the scores compare against.
    pub reference: Option<u64>,
}

/// How [`Evif::bits_per_pixel`] got its size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Bpp {
    /// From the interior bitstream in `BITS`.
    Interior(f64),
    /// From the `clam` chunk: a synthetic distortion's pretend size.
    Claimed(f64),
}

/// An unrecognized ancillary chunk, kept so rewriting a file preserves it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unknown {
    pub tag: FourCc,
    pub data: Vec<u8>,
}

/// One EVIF file in memory.
#[derive(Debug, Clone, PartialEq)]
pub struct Evif {
    pub header: Header,
    pub bits: Option<Bitstream>,
    /// PNG bytes of the rendition (`PIXL` format `"png "`).
    pub png: Vec<u8>,
    pub digests: Digests,
    pub claimed_bytes: Option<u64>,
    pub source: Option<Source>,
    pub recipe: Option<Recipe>,
    pub scores: Option<Scores>,
    pub unknown: Vec<Unknown>,
}

impl Evif {
    /// A file with the required chunks and nothing else.
    pub fn new(header: Header, png: Vec<u8>, distorted_digest: u64) -> Self {
        Self {
            header,
            bits: None,
            png,
            digests: Digests {
                distorted: distorted_digest,
                reference: None,
            },
            claimed_bytes: None,
            source: None,
            recipe: None,
            scores: None,
            unknown: Vec::new(),
        }
    }

    /// Interior bits per pixel, or the claimed size when there is no bitstream.
    pub fn bits_per_pixel(&self) -> Option<Bpp> {
        let px = self.header.pixels() as f64;
        if let Some(b) = &self.bits {
            Some(Bpp::Interior(b.data.len() as f64 * 8.0 / px))
        } else {
            self.claimed_bytes
                .map(|c| Bpp::Claimed(c as f64 * 8.0 / px))
        }
    }

    /// Serialize to bytes.
    pub fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        self.header.validate()?;
        let mut out = Vec::with_capacity(
            self.png.len() + self.bits.as_ref().map_or(0, |b| b.data.len()) + 256,
        );
        out.extend_from_slice(&SIGNATURE);
        out.extend_from_slice(&[MAJOR, MINOR, 0, 0]);

        let h = &self.header;
        let mut head = [0u8; 12];
        head[0..4].copy_from_slice(&h.width.to_le_bytes());
        head[4..8].copy_from_slice(&h.height.to_le_bytes());
        head[8] = h.layout as u8;
        head[9] = h.sample as u8;
        head[10] = 1; // sRGB
        put_chunk(&mut out, *b"HEAD", &[&head])?;

        if let Some(b) = &self.bits {
            put_chunk(&mut out, *b"BITS", &[&b.codec, &b.data])?;
        }
        put_chunk(&mut out, *b"PIXL", &[b"png ", &self.png])?;

        let d = &self.digests;
        let mut digs = Vec::with_capacity(19);
        digs.push(if d.reference.is_some() { 2 } else { 1 });
        digs.push(0);
        digs.extend_from_slice(&d.distorted.to_le_bytes());
        if let Some(r) = d.reference {
            digs.push(1);
            digs.extend_from_slice(&r.to_le_bytes());
        }
        put_chunk(&mut out, *b"DIGS", &[&digs])?;

        if let Some(c) = self.claimed_bytes {
            put_chunk(&mut out, *b"clam", &[&c.to_le_bytes()])?;
        }
        if let Some(s) = &self.source {
            put_chunk(&mut out, *b"srce", &[&meta::to_json(s)?])?;
        }
        if let Some(r) = &self.recipe {
            put_chunk(&mut out, *b"knob", &[&meta::to_json(r)?])?;
        }
        if let Some(s) = &self.scores {
            put_chunk(&mut out, *b"scor", &[&meta::to_json(s)?])?;
        }
        for u in &self.unknown {
            if !u.tag[0].is_ascii_lowercase() || is_known(u.tag) {
                return Err(Error::Corrupt(
                    "unknown chunk must be an unrecognized ancillary tag",
                ));
            }
            put_chunk(&mut out, u.tag, &[&u.data])?;
        }

        let seal = zenhash::xxh3_64(&out);
        put_chunk(&mut out, *b"SEAL", &[&seal.to_le_bytes()])?;
        Ok(out)
    }

    /// Parse and verify a complete file.
    pub fn from_bytes(data: &[u8]) -> Result<Self, Error> {
        if data.len() < HEADER_LEN || data[..8] != SIGNATURE {
            return Err(Error::Corrupt("not an EVIF file"));
        }
        if data[8] != MAJOR {
            return Err(Error::UnsupportedVersion(data[8]));
        }
        if data[10] != 0 || data[11] != 0 {
            return Err(Error::Corrupt("reserved header bytes are nonzero"));
        }

        let mut header = None;
        let mut bits = None;
        let mut png = None;
        let mut digests = None;
        let mut claimed_bytes = None;
        let mut source = None;
        let mut recipe = None;
        let mut scores = None;
        let mut unknown: Vec<Unknown> = Vec::new();
        let mut sealed = false;

        let mut pos = HEADER_LEN;
        let mut first = true;
        while pos < data.len() {
            if sealed {
                return Err(Error::Corrupt("bytes after SEAL"));
            }
            if data.len() - pos < 8 {
                return Err(Error::Corrupt("truncated chunk header"));
            }
            let tag: FourCc = data[pos..pos + 4].try_into().unwrap();
            if !tag.iter().all(u8::is_ascii_alphabetic) {
                return Err(Error::Corrupt("chunk tag is not ASCII letters"));
            }
            let len = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap()) as usize;
            let start = pos + 8;
            if data.len() - start < len {
                return Err(Error::Corrupt("chunk runs past end of file"));
            }
            let body = &data[start..start + len];
            if first && tag != *b"HEAD" {
                return Err(Error::Corrupt("HEAD is not the first chunk"));
            }
            first = false;

            match &tag {
                b"HEAD" => {
                    once(&header)?;
                    header = Some(parse_head(body)?);
                }
                b"BITS" => {
                    once(&bits)?;
                    if body.len() < 4 {
                        return Err(Error::Corrupt("BITS too short"));
                    }
                    bits = Some(Bitstream {
                        codec: body[..4].try_into().unwrap(),
                        data: body[4..].to_vec(),
                    });
                }
                b"PIXL" => {
                    once(&png)?;
                    if body.len() < 4 {
                        return Err(Error::Corrupt("PIXL too short"));
                    }
                    if body[..4] != *b"png " {
                        return Err(Error::Unsupported(body[..4].try_into().unwrap()));
                    }
                    png = Some(body[4..].to_vec());
                }
                b"DIGS" => {
                    once(&digests)?;
                    digests = Some(parse_digs(body)?);
                }
                b"clam" => {
                    once(&claimed_bytes)?;
                    let b: [u8; 8] = body
                        .try_into()
                        .map_err(|_| Error::Corrupt("clam is not 8 bytes"))?;
                    claimed_bytes = Some(u64::from_le_bytes(b));
                }
                b"srce" => {
                    once(&source)?;
                    source = Some(meta::from_json(body)?);
                }
                b"knob" => {
                    once(&recipe)?;
                    recipe = Some(meta::from_json(body)?);
                }
                b"scor" => {
                    once(&scores)?;
                    scores = Some(meta::from_json(body)?);
                }
                b"SEAL" => {
                    let b: [u8; 8] = body
                        .try_into()
                        .map_err(|_| Error::Corrupt("SEAL is not 8 bytes"))?;
                    if u64::from_le_bytes(b) != zenhash::xxh3_64(&data[..pos]) {
                        return Err(Error::Corrupt("SEAL digest mismatch"));
                    }
                    sealed = true;
                }
                _ if tag[0].is_ascii_uppercase() => return Err(Error::Unsupported(tag)),
                _ => {
                    if unknown.iter().any(|u| u.tag == tag) {
                        return Err(Error::Corrupt("duplicate chunk"));
                    }
                    unknown.push(Unknown {
                        tag,
                        data: body.to_vec(),
                    });
                }
            }
            pos = start + len;
        }

        if !sealed {
            return Err(Error::Corrupt("missing SEAL (truncated?)"));
        }
        Ok(Self {
            header: header.ok_or(Error::Corrupt("missing HEAD"))?,
            bits,
            png: png.ok_or(Error::Corrupt("missing PIXL"))?,
            digests: digests.ok_or(Error::Corrupt("missing DIGS"))?,
            claimed_bytes,
            source,
            recipe,
            scores,
            unknown,
        })
    }
}

fn is_known(tag: FourCc) -> bool {
    matches!(
        &tag,
        b"HEAD" | b"BITS" | b"PIXL" | b"DIGS" | b"clam" | b"srce" | b"knob" | b"scor" | b"SEAL"
    )
}

fn once<T>(slot: &Option<T>) -> Result<(), Error> {
    if slot.is_some() {
        Err(Error::Corrupt("duplicate chunk"))
    } else {
        Ok(())
    }
}

fn put_chunk(out: &mut Vec<u8>, tag: FourCc, parts: &[&[u8]]) -> Result<(), Error> {
    let len: usize = parts.iter().map(|p| p.len()).sum();
    let len = u32::try_from(len).map_err(|_| Error::Corrupt("chunk over 4 GiB"))?;
    out.extend_from_slice(&tag);
    out.extend_from_slice(&len.to_le_bytes());
    for p in parts {
        out.extend_from_slice(p);
    }
    Ok(())
}

fn parse_head(b: &[u8]) -> Result<Header, Error> {
    if b.len() != 12 {
        return Err(Error::Corrupt("HEAD is not 12 bytes"));
    }
    let header = Header {
        width: u32::from_le_bytes(b[0..4].try_into().unwrap()),
        height: u32::from_le_bytes(b[4..8].try_into().unwrap()),
        layout: Layout::from_u8(b[8]).ok_or(Error::Corrupt("unknown layout"))?,
        sample: Sample::from_u8(b[9]).ok_or(Error::Corrupt("unknown sample type"))?,
    };
    if b[10] != 1 {
        return Err(Error::Corrupt("unknown transfer"));
    }
    if b[11] != 0 {
        return Err(Error::Corrupt("reserved HEAD byte is nonzero"));
    }
    header.validate()?;
    Ok(header)
}

fn parse_digs(b: &[u8]) -> Result<Digests, Error> {
    let count = *b.first().ok_or(Error::Corrupt("empty DIGS"))? as usize;
    if count == 0 || count > 8 || b.len() != 1 + 9 * count {
        return Err(Error::Corrupt("bad DIGS length"));
    }
    let mut distorted = None;
    let mut reference = None;
    for e in b[1..].as_chunks::<9>().0 {
        let d = u64::from_le_bytes(e[1..9].try_into().unwrap());
        let slot = match e[0] {
            0 => &mut distorted,
            1 => &mut reference,
            _ => return Err(Error::Corrupt("unknown digest role")),
        };
        if slot.replace(d).is_some() {
            return Err(Error::Corrupt("duplicate digest role"));
        }
    }
    Ok(Digests {
        distorted: distorted.ok_or(Error::Corrupt("DIGS lacks the distorted digest"))?,
        reference,
    })
}
