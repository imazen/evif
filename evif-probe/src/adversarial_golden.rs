// Golden score bits from imazen/zensim aad4c311d850defb8d3224bb2f963779bfdd1123,
// zensim/tests/profiles.rs. Requires the two exact pinned model files.
include!("adversarial_fixtures/pairs.rs");
#[test]
fn research_adapters_match_served_revision7_golden_bits() {
    use crate::judge::Judge;
    let profiles = [super::Research::new(false), super::Research::new(true)];
    let pairs = split_parity_pairs();
    let golden: &[(usize, &str, u64)] = &[
        (0, "64x64-photo-noise", 0x4052aad69a938ab2),
        (1, "64x64-photo-noise", 0x40523aeeb9e7c6a8),
        (0, "64x64-edges-blur", 0x401e2fe329bdc22c),
        (1, "64x64-edges-blur", 0x403892a4cc5e23a3),
        (0, "65x63-text-blocks", 0xc04bb7824bf08dbe),
        (1, "65x63-text-blocks", 0x40388e003c6fccd9),
        (0, "128x96-photo-posterize", 0x405219d5ef2fb40e),
        (1, "128x96-photo-posterize", 0x4051084784f15682),
        (0, "200x150-edges-shift", 0x4056be19206d8147),
        (1, "200x150-edges-shift", 0x40560fdf8baea128),
        (0, "200x150-photo-blur", 0x404a91a05f6040e3),
        (1, "200x150-photo-blur", 0x4047794a4cb413eb),
        (0, "256x256-text-noise", 0x404807234bba5a54),
        (1, "256x256-text-noise", 0x40478cc06a650371),
        (0, "256x256-photo-pixels", 0x4058f426ec4d2c2d),
        (1, "256x256-photo-pixels", 0x4058f83453851038),
        (0, "333x251-edges-blocks", 0xc04a598d9a2688a1),
        (1, "333x251-edges-blocks", 0x40314e52d5f1ed6a),
        (0, "333x251-photo-identity", 0x4059000000000000),
        (1, "333x251-photo-identity", 0x4059000000000000),
        (0, "16x16-photo-noise", 0x4054c3e7e75a28dc),
        (1, "16x16-photo-noise", 0x40548bd9cea66699),
        (0, "31x17-text-shift", 0x405349e63d8450b3),
        (1, "31x17-text-shift", 0x4051211124b16b76),
        (0, "512x384-photo-blocks", 0x404f49b428470fbb),
        (1, "512x384-photo-blocks", 0x404ee3dc61c36fcf),
        (0, "1024x768-photo-noise", 0x404cdcd0363f2beb),
        (1, "1024x768-photo-noise", 0x4047dea51aa2d7b6),
    ];
    for &(profile, name, expected) in golden {
        let p = pairs.iter().find(|p| p.id == name).unwrap();
        let r =
            crate::image::Rgb8Image::new(p.width as u32, p.height as u32, p.src.clone()).unwrap();
        let d =
            crate::image::Rgb8Image::new(p.width as u32, p.height as u32, p.dst.clone()).unwrap();
        let got = profiles[profile].score(&r, &d).unwrap()[0].value;
        assert_eq!(
            got.to_bits(),
            expected,
            "{} {name}: {got}",
            profiles[profile].name()
        );
    }
}
