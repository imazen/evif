//! Packed sRGB RGB8 images: PNG in, PNG out, pixel digests.

use enough::Unstoppable;
use evif::{Header, Layout, Sample, pixel_digest_u8};
use zenpixels_convert::PixelBufferConvertTypedExt;

use crate::Res;

/// A tightly packed 8-bit sRGB RGB raster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgb8Image {
    pub width: u32,
    pub height: u32,
    /// `width * height * 3` bytes, rows top to bottom.
    pub data: Vec<u8>,
}

impl Rgb8Image {
    pub fn new(width: u32, height: u32, data: Vec<u8>) -> Res<Self> {
        if data.len() != width as usize * height as usize * 3 {
            return Err(format!(
                "raster is {} bytes, expected {}x{}x3",
                data.len(),
                width,
                height
            ));
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }

    pub fn pixels(&self) -> u64 {
        u64::from(self.width) * u64::from(self.height)
    }

    pub fn header(&self) -> Header {
        Header::new(self.width, self.height, Layout::Rgb, Sample::U8)
    }

    /// Row stride in bytes.
    pub fn stride(&self) -> usize {
        self.width as usize * 3
    }

    /// The `DIGS` digest of this raster.
    pub fn digest(&self) -> u64 {
        pixel_digest_u8(
            self.width,
            self.height,
            Layout::Rgb,
            &self.data,
            self.stride(),
        )
        .expect("packed raster matches its dimensions")
    }

    /// Pixels as `[r, g, b]` triples.
    pub fn triples(&self) -> &[[u8; 3]] {
        self.data.as_chunks::<3>().0
    }

    /// Pixels as `rgb::RGB8` for the imgref-based metrics.
    pub fn imgref(&self) -> imgref::ImgRef<'_, rgb::RGB8> {
        let px: &[rgb::RGB8] = bytemuck_cast(self.triples());
        imgref::ImgRef::new(px, self.width as usize, self.height as usize)
    }

    /// Decode a PNG of any color type to RGB8. Alpha is dropped; the metrics
    /// here score opaque sRGB.
    pub fn from_png(bytes: &[u8]) -> Res<Self> {
        let out = zenpng::decode(bytes, &zenpng::PngDecodeConfig::default(), &Unstoppable)
            .map_err(|e| format!("png decode: {e}"))?;
        let rgb = out.pixels.to_rgb8();
        let view = rgb.as_slice();
        let (w, h) = (rgb.width(), rgb.height());
        let row = w as usize * 3;
        let mut data = Vec::with_capacity(row * h as usize);
        for y in 0..h {
            data.extend_from_slice(&view.row(y)[..row]);
        }
        Self::new(w, h, data)
    }

    /// Encode as a lossless PNG with no color chunks (legacy wire spec section 4).
    pub fn to_png(&self) -> Res<Vec<u8>> {
        let bytes = zenpng::encode_rgb8(
            self.imgref(),
            None,
            &zenpng::EncodeConfig::default(),
            &Unstoppable,
            &Unstoppable,
        )
        .map_err(|e| format!("png encode: {e}"))?;
        // The rendition must read back bit-exact; a PNG writer that changed
        // pixels would silently invalidate every score in the file.
        let back = Self::from_png(&bytes)?;
        if back != *self {
            return Err("png round trip changed pixels".into());
        }
        Ok(bytes)
    }
}

/// `[u8; 3]` and `rgb::RGB8` have the same layout; the `rgb` crate exposes
/// the cast through bytemuck so no unsafe is needed here.
fn bytemuck_cast(px: &[[u8; 3]]) -> &[rgb::RGB8] {
    rgb::bytemuck::cast_slice(px)
}
