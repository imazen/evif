use alloc::vec::Vec;
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};

use super::*;

fn buffer(len: usize) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    out.try_reserve_exact(len)
        .map_err(|_| Error::Limit("metadata allocation"))?;
    out.resize(len, 0);
    Ok(out)
}

fn read_state<R: Read>(reader: &mut R, limits: Limits) -> Result<State, Error> {
    let mut header = [0; 16];
    reader.read_exact(&mut header)?;
    file_header(&header)?;
    let mut state = State::new(limits);
    loop {
        let mut header = [0; 12];
        // read_exact retries Interrupted; an EOF only at a record boundary is valid.
        match reader.read_exact(&mut header[..1]) {
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            result => result?,
        }
        reader.read_exact(&mut header[1..])?;
        let tag = header[..4].try_into().unwrap();
        let len = u64::from_le_bytes(header[4..].try_into().unwrap());
        let offset = state
            .index
            .file_end
            .checked_add(12)
            .ok_or(Error::Invalid("file offset overflow"))?;
        state.start(tag, len, offset)?;
        let span = Span {
            offset,
            length: len,
        };
        if is_json(tag) {
            let size = usize::try_from(len).map_err(|_| Error::Limit("JSON size"))?;
            let mut body = buffer(size)?;
            reader.read_exact(&mut body)?;
            state.json(tag, &body)?;
        } else {
            let mut prefix = [0; 8];
            let bits = tag == *b"BITS";
            if bits {
                check(len > 8, "short BITS")?;
                reader.read_exact(&mut prefix)?;
            }
            let hash =
                if matches!(&tag, b"PNGF" | b"IMIJ") && state.expected_payload_hash().is_some() {
                    Some(Sha256::new())
                } else {
                    None
                };
            let actual = transfer(
                reader,
                &mut std::io::sink(),
                len - if bits { 8 } else { 0 },
                hash,
            )?;
            if matches!(&tag, b"PNGF" | b"IMIJ") {
                state.payload(span, actual)?;
            } else if bits {
                state.bits(&prefix, span)?;
            } else {
                state.index.ancillary.push(Ancillary { tag, payload: span });
            }
        }
    }
    check(
        state.ended && state.pending.is_none(),
        "missing MEND or pending payload",
    )?;
    Ok(state)
}

/// Scan from the beginning of a file/stream, retaining bounded metadata and
/// absolute payload spans. Reads payloads in 64 KiB blocks. Unknown ancillary
/// payloads are consumed without interpretation. Does not fetch external URIs.
pub fn read<R: Read>(reader: &mut R, limits: Limits) -> Result<Index, Error> {
    read_state(reader, limits)?.finish()
}

fn transfer<R: Read, W: Write>(
    source: &mut R,
    dest: &mut W,
    mut len: u64,
    mut hash: Option<Sha256>,
) -> Result<Option<Hash>, Error> {
    let mut buf = [0; 65536];
    while len != 0 {
        let size = len.min(buf.len() as u64) as usize;
        source.read_exact(&mut buf[..size])?;
        if let Some(h) = &mut hash {
            h.update(&buf[..size]);
        }
        dest.write_all(&buf[..size])?;
        len -= size as u64;
    }
    Ok(hash.map(|h| Hash::sha256(h.finalize().into())))
}

/// Sequential writer: no seek/backpatch and no whole-payload buffering. A failed
/// record write poisons the writer so callers cannot complete a partial file.
/// A complete media section ends with `finish_media`; scores may follow.
pub struct Writer<W: Write> {
    sink: W,
    state: State,
    poisoned: bool,
}
impl<W: Write> Writer<W> {
    pub fn new(mut sink: W, limits: Limits) -> Result<Self, Error> {
        sink.write_all(&HEADER)?;
        Ok(Self {
            sink,
            state: State::new(limits),
            poisoned: false,
        })
    }
    fn usable(&self) -> Result<(), Error> {
        check(!self.poisoned, "writer is poisoned")
    }
    /// Write a complete bounded record, including unknown ancillary records.
    /// Use `segment` to stream a large PNGF/IMIJ payload.
    pub fn record(&mut self, tag: [u8; 4], body: &[u8]) -> Result<(), Error> {
        self.usable()?;
        self.poisoned = true;
        let offset = self
            .state
            .index
            .file_end
            .checked_add(12)
            .ok_or(Error::Invalid("file offset overflow"))?;
        let span = Span {
            offset,
            length: body.len() as u64,
        };
        self.state.start(tag, span.length, offset)?;
        if is_json(tag) {
            self.state.json(tag, body)?;
        } else if matches!(&tag, b"PNGF" | b"IMIJ") {
            let actual = self.state.expected_payload_hash().map(|_| Hash::of(body));
            self.state.payload(span, actual)?;
        } else if tag == *b"BITS" {
            self.state
                .bits(body.get(..8).ok_or(Error::Invalid("short BITS"))?, span)?;
        } else {
            self.state
                .index
                .ancillary
                .push(Ancillary { tag, payload: span });
        }
        self.sink.write_all(&record_header(tag, span.length))?;
        self.sink.write_all(body)?;
        self.poisoned = false;
        Ok(())
    }
    pub fn asset(&mut self, asset: &Asset) -> Result<(), Error> {
        self.record(*b"ASST", &serde_json::to_vec(asset)?)
    }
    /// For embedded storage, copies exactly `length` bytes from `source`. For
    /// external or shared-page storage, length must be zero and source is not read.
    pub fn segment<R: Read>(
        &mut self,
        segment: &Segment,
        source: &mut R,
        length: u64,
    ) -> Result<(), Error> {
        self.usable()?;
        if matches!(
            segment.storage,
            Storage::External { .. } | Storage::SharedPages { .. }
        ) {
            check(length == 0, "non-owning segment cannot embed bytes")?;
            return self.record(*b"SEGM", &serde_json::to_vec(segment)?);
        }
        self.record(*b"SEGM", &serde_json::to_vec(segment)?)?;
        self.poisoned = true;
        let tag = segment.carrier.tag();
        let offset = self
            .state
            .index
            .file_end
            .checked_add(12)
            .ok_or(Error::Invalid("file offset overflow"))?;
        self.state.start(tag, length, offset)?;
        self.sink.write_all(&record_header(tag, length))?;
        let hasher = self.state.expected_payload_hash().map(|_| Sha256::new());
        let hash = transfer(source, &mut self.sink, length, hasher)?;
        self.state.payload(Span { offset, length }, hash)?;
        self.poisoned = false;
        Ok(())
    }
    /// Preserve one original encoded object for either asset, including EVIF
    /// coding (`codec = *b"evif"`). It never substitutes for a rendition.
    pub fn bitstream<R: Read>(
        &mut self,
        asset: u32,
        codec: [u8; 4],
        source: &mut R,
        length: u64,
    ) -> Result<(), Error> {
        self.usable()?;
        self.poisoned = true;
        let length = length
            .checked_add(8)
            .ok_or(Error::Invalid("BITS length overflow"))?;
        let offset = self
            .state
            .index
            .file_end
            .checked_add(12)
            .ok_or(Error::Invalid("file offset overflow"))?;
        self.state.start(*b"BITS", length, offset)?;
        let mut prefix = [0; 8];
        prefix[..4].copy_from_slice(&asset.to_le_bytes());
        prefix[4..].copy_from_slice(&codec);
        self.state.bits(&prefix, Span { offset, length })?;
        self.sink.write_all(&record_header(*b"BITS", length))?;
        self.sink.write_all(&prefix)?;
        transfer(source, &mut self.sink, length - 8, None)?;
        self.poisoned = false;
        Ok(())
    }
    /// Close immutable media and return its storage-independent identities.
    pub fn finish_media(&mut self) -> Result<Vec<MediaRoot>, Error> {
        self.usable()?;
        let roots = self.state.roots()?;
        self.record(*b"MEND", &serde_json::to_vec(&roots)?)?;
        Ok(roots)
    }
    pub fn score(&mut self, score: &ScoreRecord) -> Result<(), Error> {
        self.record(*b"scor", &serde_json::to_vec(score)?)
    }
    /// Requires completed media and no failed write; flushes the destination.
    pub fn finish(mut self) -> Result<(W, Index), Error> {
        self.usable()?;
        self.sink.flush()?;
        Ok((self.sink, self.state.finish()?))
    }
}

/// Validate an existing file, then append a score without rewriting any bytes.
/// Caller must serialize writers and open read/write (not truncating). A failed
/// append can leave a partial tail: strict readers report it; no automatic repair
/// discards bytes. The file must start at offset zero.
pub fn append_score<F: Read + Write + Seek>(
    file: &mut F,
    score: &ScoreRecord,
    limits: Limits,
) -> Result<(), Error> {
    append_record(file, *b"scor", &serde_json::to_vec(score)?, limits)
}

/// Append a validated scoring or ancillary record, preserving its body bytes
/// exactly. Same caller synchronization requirements as [`append_score`].
pub fn append_record<F: Read + Write + Seek>(
    file: &mut F,
    tag: [u8; 4],
    body: &[u8],
    limits: Limits,
) -> Result<(), Error> {
    file.seek(SeekFrom::Start(0))?;
    let state = read_state(file, limits)?;
    let mut writer = Writer {
        sink: file,
        state,
        poisoned: false,
    };
    writer.record(tag, body)?;
    writer.finish()?;
    Ok(())
}
