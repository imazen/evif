//! Native-precision PQ pilot: convert primaries with zenpixels, then encode SVT.
use enough::Unstoppable;
use std::{fs, path::Path};
use zenpixels_convert::{PixelBufferConvertExt, PixelDescriptor, TransferFunction};

fn config(delta: i8, subsampling: zenavif::EncodeChromaSubsampling) -> zenavif::EncoderConfig {
    let mut knobs = zenavif::expert::SvtParams::default();
    knobs.chroma_q = Some((delta, delta));
    zenavif::EncoderConfig::new()
        .backend(zenavif::Av1Backend::Zenav1Svt)
        .quality(95.0)
        .speed(6)
        .bit_depth(zenavif::EncodeBitDepth::Ten)
        .chroma_subsampling(subsampling)
        .color_primaries(9)
        .transfer_characteristics(16)
        .threads(Some(1))
        .with_svt_params(knobs)
}
fn encode(
    img: imgref::ImgRef<'_, rgb::Rgb<u16>>,
    delta: i8,
    sub: zenavif::EncodeChromaSubsampling,
) -> Result<Vec<u8>, String> {
    zenavif::encode_rgb16(
        img,
        &config(delta, sub),
        almost_enough::StopToken::new(Unstoppable),
    )
    .map(|e| e.avif_file)
    .map_err(|e| e.to_string())
}
fn pq_png_from_avif(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let pixels = zenavif::decode_with(
        bytes,
        &zenavif::DecoderConfig::new().threads(1).prefer_8bit(false),
        &Unstoppable,
    )?;
    if pixels.descriptor() != PixelDescriptor::RGB16_BT2100_PQ {
        return Err(format!(
            "expected native RGB16 BT.2020 PQ, got {:?}",
            pixels.descriptor()
        )
        .into());
    }
    let packed = pixels.copy_to_contiguous_bytes();
    let samples: Vec<rgb::Rgb<u16>> = packed
        .as_chunks::<6>()
        .0
        .iter()
        .map(|p| {
            rgb::Rgb::new(
                u16::from_ne_bytes([p[0], p[1]]),
                u16::from_ne_bytes([p[2], p[3]]),
                u16::from_ne_bytes([p[4], p[5]]),
            )
        })
        .collect();
    let config =
        zenpng::EncodeConfig::default().with_cicp(Some(zencodec::Cicp::new(9, 16, 0, true)));
    let png = zenpng::encode_rgb16(
        imgref::ImgRef::new(&samples, pixels.width() as usize, pixels.height() as usize),
        None,
        &config,
        &Unstoppable,
        &Unstoppable,
    )?;
    let back = zenpng::PngDecoderConfig::new().decode(&png)?.into_buffer();
    if back.descriptor() != pixels.descriptor() || back.copy_to_contiguous_bytes() != packed {
        return Err("AVIF PQ rendition roundtrip changed samples or signaling".into());
    }
    Ok(png)
}
fn run(input: &Path, out: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir(out)?;
    let decoded = zenpng::PngDecoderConfig::new()
        .decode(&fs::read(input)?)?
        .into_buffer();
    let desc = decoded.descriptor();
    if desc.channel_type() != zenpixels_convert::ChannelType::U16
        || desc.layout() != zenpixels_convert::ChannelLayout::Rgb
        || desc.transfer() != TransferFunction::Pq
        || decoded.width() as u64 * decoded.height() as u64 >= 1_000_000
    {
        return Err("input must be native PQ below 1MP".into());
    }
    let converted = decoded.convert_to(PixelDescriptor::RGB16_BT2100_PQ)?;
    let bytes = converted.copy_to_contiguous_bytes();
    let samples: Vec<rgb::Rgb<u16>> = bytes
        .as_chunks::<6>()
        .0
        .iter()
        .map(|p| {
            rgb::Rgb::new(
                u16::from_ne_bytes([p[0], p[1]]),
                u16::from_ne_bytes([p[2], p[3]]),
                u16::from_ne_bytes([p[4], p[5]]),
            )
        })
        .collect();
    let img = imgref::ImgRef::new(
        &samples,
        converted.width() as usize,
        converted.height() as usize,
    );
    let png_config =
        zenpng::EncodeConfig::default().with_cicp(Some(zencodec::Cicp::new(9, 16, 0, true)));
    let reference = zenpng::encode_rgb16(img, None, &png_config, &Unstoppable, &Unstoppable)?;
    let back = zenpng::PngDecoderConfig::new()
        .decode(&reference)?
        .into_buffer();
    if back.copy_to_contiguous_bytes() != bytes || back.descriptor() != converted.descriptor() {
        return Err("PQ reference PNG roundtrip changed samples/signaling".into());
    }
    fs::write(out.join("reference-bt2020-pq.png"), reference)?;
    let refusal = encode(img, 0, zenavif::EncodeChromaSubsampling::Yuv444)
        .expect_err("pinned SVT must explicitly refuse 444");
    if !refusal.contains("only Yuv420 is implemented") {
        return Err(format!("unexpected 444 refusal: {refusal}").into());
    }
    fs::write(out.join("444-refusal.txt"), refusal)?;
    let mut rows = Vec::new();
    for delta in [
        0, 4, 8, 12, 16, 20, 24, 28, 32, 36, 40, 44, 48, 52, 56, 60, 63,
    ] {
        let avif = encode(img, delta, zenavif::EncodeChromaSubsampling::Yuv420)?;
        let name = format!("chroma-{delta:02}.avif");
        fs::write(out.join(&name), &avif)?;
        rows.push(serde_json::json!({"file":name,"bytes":avif.len(),"quality":95,"chroma_q":[delta,delta],"bit_depth":10,"cicp":[9,16,6,true],"source_descriptor":format!("{desc:?}")}));
    }
    fs::write(out.join("encodes.json"), serde_json::to_vec_pretty(&rows)?)?;
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() == 4 && args[1] == "decode" {
        if Path::new(&args[3]).exists() {
            return Err("rendition output already exists".into());
        }
        fs::write(&args[3], pq_png_from_avif(&fs::read(&args[2])?)?)?;
        return Ok(());
    }
    if args.len() != 3 {
        return Err("chroma_hdr INPUT.hdr.png NEW_OUTPUT_DIR".into());
    }
    run(Path::new(&args[1]), Path::new(&args[2]))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pq_png_decode_preserves_native_samples_and_cicp() {
        let values = vec![
            rgb::Rgb::new(32768u16, 2047, 19),
            rgb::Rgb::new(10001, 12345, 65500),
        ];
        let config =
            zenpng::EncodeConfig::default().with_cicp(Some(zencodec::Cicp::new(1, 16, 0, true)));
        let png = zenpng::encode_rgb16(
            imgref::ImgRef::new(&values, 2, 1),
            None,
            &config,
            &Unstoppable,
            &Unstoppable,
        )
        .unwrap();
        let back = zenpng::PngDecoderConfig::new()
            .decode(&png)
            .unwrap()
            .into_buffer();
        assert_eq!(back.descriptor().transfer(), TransferFunction::Pq);
        assert_eq!(
            back.descriptor().primaries,
            zenpixels_convert::ColorPrimaries::Bt709
        );
        let expected: Vec<u8> = values
            .iter()
            .flat_map(|p| [p.r, p.g, p.b])
            .flat_map(u16::to_ne_bytes)
            .collect();
        assert_eq!(back.copy_to_contiguous_bytes(), expected);
    }
    #[test]
    fn pq_primary_conversion_changes_samples_without_eight_bit_narrowing() {
        use zenpixels_convert::{ColorPrimaries, PixelBuffer};
        let red = [32768u16, 0, 0];
        let bytes = red.into_iter().flat_map(u16::to_ne_bytes).collect();
        let input = PixelBuffer::from_vec(
            bytes,
            1,
            1,
            PixelDescriptor::RGB16_BT2100_PQ.with_primaries(ColorPrimaries::Bt709),
        )
        .unwrap();
        let out = input.convert_to(PixelDescriptor::RGB16_BT2100_PQ).unwrap();
        let values: Vec<u16> = out
            .copy_to_contiguous_bytes()
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u16::from_ne_bytes([p[0], p[1]]))
            .collect();
        assert!(
            values[0] < red[0] && values[1] > 0 && values[2] > 0,
            "linear-light gamut conversion must change nonneutral PQ samples"
        );
        assert!(
            values.iter().any(|v| v % 257 != 0),
            "preserve native precision"
        );
    }
    #[test]
    fn hbd_path_accepts_real_samples_and_refuses_444() {
        let samples: Vec<_> = (0..32 * 32)
            .map(|i| rgb::Rgb::new((i * 61) as u16, (i * 37) as u16, (i * 17) as u16))
            .collect();
        let img = imgref::ImgRef::new(&samples, 32, 32);
        let a = encode(img, 0, zenavif::EncodeChromaSubsampling::Yuv420).unwrap();
        let b = encode(img, 63, zenavif::EncodeChromaSubsampling::Yuv420).unwrap();
        assert_ne!(a, b, "chroma override must affect HBD output");
        let png = pq_png_from_avif(&a).unwrap();
        let decoded = zenpng::PngDecoderConfig::new()
            .decode(&png)
            .unwrap()
            .into_buffer();
        assert_eq!(decoded.descriptor(), PixelDescriptor::RGB16_BT2100_PQ);
        assert_eq!((decoded.width(), decoded.height()), (32, 32));
        let err = encode(img, 0, zenavif::EncodeChromaSubsampling::Yuv444).unwrap_err();
        assert!(err.contains("only Yuv420 is implemented"), "{err}");
    }
}
