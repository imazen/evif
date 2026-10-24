//! Every judge must give the same bits for the same pixels, whatever buffer
//! they sit in: a file's recorded scores are only checkable if re-scoring
//! reproduces them.

use evif_probe::image::Rgb8Image;
use evif_probe::registry::Registry;
use evif_probe::rng::Rng;

fn image(seed: u64, w: u32, h: u32) -> Rgb8Image {
    let mut rng = Rng::new(seed);
    let data = (0..(w * h * 3) as usize)
        .map(|i| ((i % 251) as u8).wrapping_add((rng.unit() * 40.0) as u8))
        .collect();
    Rgb8Image::new(w, h, data).unwrap()
}

/// The same pixels in a buffer whose start address differs by `shift` bytes.
fn shifted(img: &Rgb8Image, shift: usize) -> Rgb8Image {
    let mut v = Vec::with_capacity(img.data.len() + 64);
    v.resize(shift, 0u8);
    v.extend_from_slice(&img.data);
    v.drain(..shift);
    Rgb8Image::new(img.width, img.height, v).unwrap()
}

#[test]
fn judges_are_reproducible_across_buffers() {
    let reference = image(1, 160, 128);
    let distorted = image(2, 160, 128);
    let mut failures = Vec::new();
    for judge in Registry::builtin().all_judges() {
        let base = judge.score(&reference, &distorted).unwrap();
        for shift in [1usize, 3, 8, 13, 32, 57] {
            let r = shifted(&reference, shift);
            let d = shifted(&distorted, shift);
            assert_eq!(r.data, reference.data);
            let again = judge.score(&r, &d).unwrap();
            for (a, b) in base.iter().zip(&again) {
                if a.value != b.value {
                    failures.push(format!(
                        "{} shift {shift}: {} vs {} (ptr mod 64: {} / {})",
                        a.key(),
                        a.value,
                        b.value,
                        reference.data.as_ptr() as usize % 64,
                        r.data.as_ptr() as usize % 64
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
