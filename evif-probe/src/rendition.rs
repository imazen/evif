//! Lossless storage of the scored raster. Private companions register IMIJ.
use crate::{Res, image::Rgb8Image, registry::Registry};
use evif::{Evif, Rendition, RenditionFormat};

/// A lossless reference-decode codec; never the candidate encoder in BITS.
pub trait RenditionCodec {
    fn encode(&self, image: &Rgb8Image) -> Res<Vec<u8>>;
    fn decode(&self, data: &[u8]) -> Res<Rgb8Image>;
}

pub(crate) struct Png;
impl RenditionCodec for Png {
    fn encode(&self, image: &Rgb8Image) -> Res<Vec<u8>> {
        image.to_png()
    }
    fn decode(&self, data: &[u8]) -> Res<Rgb8Image> {
        Rgb8Image::from_png(data)
    }
}

pub(crate) fn parse(value: &str) -> Res<RenditionFormat> {
    match value {
        "png" => Ok(RenditionFormat::Png),
        "imij" => Ok(RenditionFormat::Imij),
        _ => Err(
            "reference decode must be png or imij; candidate compression belongs in BITS".into(),
        ),
    }
}

pub(crate) fn encode(
    registry: &Registry,
    format: RenditionFormat,
    image: &Rgb8Image,
) -> Res<Rendition> {
    let codec = registry
        .rendition_codec(format)
        .ok_or_else(|| format!("{format:?} reference codec is not registered in this binary"))?;
    let data = codec.encode(image)?;
    if codec.decode(&data)? != *image {
        return Err("reference encoding changed scored pixels".into());
    }
    Ok(Rendition { format, data })
}

pub(crate) fn decode(registry: &Registry, file: &Evif) -> Res<Rgb8Image> {
    let codec = registry
        .rendition_codec(file.rendition.format)
        .ok_or_else(|| {
            format!(
                "{:?} reference codec is not registered in this binary",
                file.rendition.format
            )
        })?;
    let image = codec.decode(&file.rendition.data)?;
    if image.header() != file.header {
        return Err("reference decode dimensions/layout/sample differ from HEAD".into());
    }
    if image.digest() != file.digests.distorted {
        return Err("reference decode digest differs from DIGS".into());
    }
    Ok(image)
}

pub(crate) fn configure(
    registry: &Registry,
    file: &mut Evif,
    format: RenditionFormat,
    omit: bool,
) -> Res<()> {
    let image = decode(registry, file)?;
    if file.rendition.format != format {
        file.rendition = encode(registry, format, &image)?;
    }
    if omit {
        file.omit_bits();
    }
    Ok(())
}

// Codec tags are untrusted file data, never path components.
pub(crate) fn bitstream_extension(codec: &[u8; 4]) -> String {
    if codec == b"evif" {
        return "evifbits".into();
    }
    let text = String::from_utf8_lossy(codec);
    let name = text.trim();
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric()) {
        "bin".into()
    } else {
        name.to_ascii_lowercase()
    }
}
