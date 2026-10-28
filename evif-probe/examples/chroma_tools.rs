//! Small transport helpers using the same pixels and judges as EVIF.
use evif_probe::{
    image::Rgb8Image,
    judge::{Judge, Ssim2},
};
use std::fs;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a: Vec<_> = std::env::args().collect();
    match a.get(1).map(String::as_str) {
        Some("verify-scores") if a.len() == 3 => {
            #[derive(serde::Deserialize)]
            struct Row {
                original: String,
                scored: String,
                value: f64,
                implementation: String,
            }
            let rows: Vec<Row> = serde_json::from_slice(&fs::read(&a[2])?)?;
            for row in &rows {
                let original = evif::Evif::from_bytes(&fs::read(&row.original)?)?;
                let mut scored = evif::Evif::from_bytes(&fs::read(&row.scored)?)?;
                let scores = &scored.scores.as_ref().ok_or("scores missing")?.scores;
                if scores.len() != 1
                    || scores[0].value.to_bits() != row.value.to_bits()
                    || scores[0].implementation.as_deref() != Some(row.implementation.as_str())
                {
                    return Err(format!("score roundtrip differs: {}", row.scored).into());
                }
                scored.scores = original.scores.clone();
                if scored != original {
                    return Err(
                        format!("score import changed media or recipe: {}", row.scored).into(),
                    );
                }
            }
            println!("{} exact score/media roundtrips verified", rows.len());
        }
        Some("decode-jpeg") if a.len() == 4 => {
            let dec =
                zenjpeg::decoder::Decoder::new().decode(&fs::read(&a[2])?, enough::Unstoppable)?;
            let (w, h) = dec.dimensions();
            let pixels = dec.pixels_u8().ok_or("JPEG produced no u8 pixels")?;
            fs::write(&a[3], Rgb8Image::new(w, h, pixels.to_vec())?.to_png()?)?;
        }
        Some("crop") if a.len() == 6 => {
            let im = Rgb8Image::from_png(&fs::read(&a[2])?)?;
            let size: u32 = a[4].parse()?;
            let offset: u32 = a[5].parse()?;
            if size == 0 || size > im.width || size > im.height || offset > 1 {
                return Err("crop must fit; offset 0=center, 1=top-left".into());
            }
            let (x, y) = if offset == 0 {
                ((im.width - size) / 2, (im.height - size) / 2)
            } else {
                (0, 0)
            };
            let mut data = Vec::with_capacity(size as usize * size as usize * 3);
            for row in y..y + size {
                let start = row as usize * im.stride() + x as usize * 3;
                data.extend_from_slice(&im.data[start..start + size as usize * 3]);
            }
            let cropped = Rgb8Image::new(size, size, data)?;
            fs::write(&a[3], cropped.to_png()?)?;
        }
        Some("ssim") if a.len() == 4 => {
            let r = Rgb8Image::from_png(&fs::read(&a[2])?)?;
            let d = Rgb8Image::from_png(&fs::read(&a[3])?)?;
            println!("{}", serde_json::to_string(&Ssim2.score(&r, &d)?)?);
        }
        _ => {
            return Err(
                "chroma_tools crop INPUT OUTPUT SIZE POSITION | ssim REF.png DIST.png".into(),
            );
        }
    }
    Ok(())
}
