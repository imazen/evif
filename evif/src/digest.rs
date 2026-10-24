//! Pixel digests (`DIGS`, SPEC section 5).

use zenhash::Xxh3;

use crate::{Error, Layout, Sample};

const DOMAIN: &[u8; 16] = b"EVIF/pdig/v1\0\0\0\0";

fn start(width: u32, height: u32, layout: Layout, sample: Sample) -> Xxh3 {
    let mut h = Xxh3::new();
    h.update(DOMAIN);
    h.update(&width.to_le_bytes());
    h.update(&height.to_le_bytes());
    h.update(&[layout as u8, sample as u8]);
    h
}

fn check(
    width: u32,
    height: u32,
    layout: Layout,
    len: usize,
    stride: usize,
) -> Result<usize, Error> {
    let row = width as usize * layout.channels();
    if stride < row {
        return Err(Error::Corrupt("stride shorter than a row"));
    }
    let need = (height as usize).saturating_sub(1) * stride + row;
    if height == 0 || width == 0 || len < need {
        return Err(Error::Corrupt("raster smaller than its dimensions"));
    }
    Ok(row)
}

/// Digest of an 8-bit raster. `stride` is in samples between row starts.
pub fn pixel_digest_u8(
    width: u32,
    height: u32,
    layout: Layout,
    raster: &[u8],
    stride: usize,
) -> Result<u64, Error> {
    let row = check(width, height, layout, raster.len(), stride)?;
    let mut h = start(width, height, layout, Sample::U8);
    for y in 0..height as usize {
        h.update(&raster[y * stride..y * stride + row]);
    }
    Ok(h.digest())
}

/// Digest of a 16-bit raster. `stride` is in samples between row starts.
pub fn pixel_digest_u16(
    width: u32,
    height: u32,
    layout: Layout,
    raster: &[u16],
    stride: usize,
) -> Result<u64, Error> {
    let row = check(width, height, layout, raster.len(), stride)?;
    let mut h = start(width, height, layout, Sample::U16);
    let mut buf = [0u8; 512];
    for y in 0..height as usize {
        for chunk in raster[y * stride..y * stride + row].chunks(buf.len() / 2) {
            for (dst, s) in buf.as_chunks_mut::<2>().0.iter_mut().zip(chunk) {
                dst.copy_from_slice(&s.to_le_bytes());
            }
            h.update(&buf[..chunk.len() * 2]);
        }
    }
    Ok(h.digest())
}
