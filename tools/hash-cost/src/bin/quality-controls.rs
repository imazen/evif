//! Reproduce fixed-public-secret multiplication collapse, not a statistical suite.
//! Analysis: https://liams.website/articles/seed-independent-collisions-on-wyhash-and-rapidhash
use std::collections::BTreeSet;

fn main() {
    let secrets = rapidhash::v3::DEFAULT_RAPID_SECRETS;
    for length in [32usize, 1024 * 1024] {
        let mut rapid = BTreeSet::new();
        let mut protected = BTreeSet::new();
        let mut muse = BTreeSet::new();
        let mut xxh = BTreeSet::new();
        for variant in 0u8..16 {
            let mut input = vec![variant; length];
            let remainder = ((length - 1) % 112 + 1) as u64;
            input[length - 16..length - 8]
                .copy_from_slice(&(secrets.secrets[1] ^ remainder).to_le_bytes());
            let mut h = rapidhash::v3::RapidStreamHasherV3::new(&secrets);
            h.write(&input);
            rapid.insert(h.finish());
            let mut h = rapidhash::v3::RapidStreamHasherInlineV3::<true, true>::new(&secrets);
            h.write(&input);
            protected.insert(h.finish());
            muse.insert(museair::hash128(&input, 0, 0));
            xxh.insert(zenhash::xxh3_128(&input));
        }
        assert_eq!(rapid.len(), 1, "published collapse did not reproduce");
        assert_eq!(protected.len(), 16, "protected-mode control collided");
        assert_eq!(muse.len(), 16, "MuseAir control collided");
        assert_eq!(xxh.len(), 16, "XXH3 control collided");
        println!(
            "bytes={length} distinct_inputs=16 rapid_distinct={} protected_distinct={} muse128_distinct={} xxh3_128_distinct={} rapid_digest={:016x}",
            rapid.len(),
            protected.len(),
            muse.len(),
            xxh.len(),
            rapid.first().unwrap()
        );
    }
}
