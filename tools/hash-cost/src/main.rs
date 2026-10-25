//! Single-thread streaming hash comparison. No codec, file I/O, or data
//! generation inside timed calls. Default CPU dispatch; no target-cpu=native.
use sha2::Digest;
use std::{hint::black_box, path::PathBuf, sync::Arc, time::Duration};
use zenbench::{Suite, Throughput};

#[derive(Clone, Copy, Debug)]
enum Algorithm {
    Xxh3,
    Rapid,
    Muse,
    Sha256,
    Blake3,
    Kt128,
    Kt256,
    Ascon,
}
const ALGORITHMS: [Algorithm; 8] = [
    Algorithm::Xxh3,
    Algorithm::Rapid,
    Algorithm::Muse,
    Algorithm::Sha256,
    Algorithm::Blake3,
    Algorithm::Kt128,
    Algorithm::Kt256,
    Algorithm::Ascon,
];

fn hash(algorithm: Algorithm, bytes: &[u8], chunk: usize) -> [u8; 32] {
    let mut out = [0; 32];
    match algorithm {
        Algorithm::Xxh3 => {
            let mut h = zenhash::Xxh3::new();
            for part in bytes.chunks(chunk) {
                h.update(part);
            }
            out[..8].copy_from_slice(&h.digest().to_le_bytes());
        }
        Algorithm::Rapid => {
            let mut h =
                rapidhash::v3::RapidStreamHasherV3::new(&rapidhash::v3::DEFAULT_RAPID_SECRETS);
            for part in bytes.chunks(chunk) {
                h.write(part);
            }
            out[..8].copy_from_slice(&h.finish().to_le_bytes());
        }
        Algorithm::Muse => {
            let mut h = museair::Hasher128::with_seed(0, 0);
            for part in bytes.chunks(chunk) {
                h.write(part);
            }
            out[..16].copy_from_slice(&h.finish128().to_le_bytes());
        }
        Algorithm::Sha256 => {
            let mut h = sha2::Sha256::new();
            for part in bytes.chunks(chunk) {
                h.update(part);
            }
            out.copy_from_slice(&h.finalize());
        }
        Algorithm::Blake3 => {
            let mut h = blake3::Hasher::new();
            for part in bytes.chunks(chunk) {
                h.update(part);
            }
            out.copy_from_slice(h.finalize().as_bytes());
        }
        Algorithm::Kt128 => {
            use k12::{ExtendableOutput, Update, XofReader};
            let mut h = k12::Kt128::default();
            for part in bytes.chunks(chunk) {
                h.update(part);
            }
            h.finalize_xof().read(&mut out);
        }
        Algorithm::Kt256 => {
            use k12::{ExtendableOutput, Update, XofReader};
            let mut h = k12::Kt256::default();
            for part in bytes.chunks(chunk) {
                h.update(part);
            }
            h.finalize_xof().read(&mut out);
        }
        Algorithm::Ascon => {
            let mut h = ascon_hash256::AsconHash256::new();
            for part in bytes.chunks(chunk) {
                h.update(part);
            }
            out.copy_from_slice(&h.finalize());
        }
    }
    out
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn validate() {
    // Published examples from the algorithm/crate documentation.
    assert_eq!(
        hex(&hash(Algorithm::Sha256, b"abc", 1)),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(
        hex(&hash(Algorithm::Blake3, b"", 1)),
        "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
    );
    assert_eq!(
        hex(&hash(Algorithm::Kt128, b"abc", 1)[..20]),
        "ab174f328c55a5510b0b209791bf8b60e801a7cf"
    );
    assert_eq!(
        hex(&hash(Algorithm::Ascon, b"some bytes", 1)),
        "e909c2f6da9cb3028423265c8f23fc2d26bfc0f3db704683ef16b787a945ed68"
    );
    let bytes: Vec<u8> = (0usize..20000)
        .map(|i| (i.wrapping_mul(37) ^ (i >> 8)) as u8)
        .collect();
    for algorithm in ALGORITHMS {
        let expected = hash(algorithm, &bytes, bytes.len());
        for chunk in [1, 63, 64, 65, 1023, 1024, 8191, 8192, 8193] {
            assert_eq!(
                hash(algorithm, &bytes, chunk),
                expected,
                "{algorithm:?}, chunk={chunk}"
            );
        }
    }
}

fn benchmarks(suite: &mut Suite) {
    for size in [64usize, 4096, 1024 * 1024, 64 * 1024 * 1024] {
        let bytes: Arc<[u8]> = (0..size)
            .map(|i| (i.wrapping_mul(37) ^ (i >> 8)) as u8)
            .collect();
        for chunk in [size, 16 * 1024] {
            if chunk > size {
                continue;
            }
            suite.group(format!("bytes-{size}/update-{chunk}"), |g| {
                g.throughput(Throughput::Bytes(size as u64));
                g.config()
                    .min_rounds(30)
                    .max_rounds(30)
                    .warmup_time(Duration::from_millis(100));
                g.config().auto_rounds = false;
                for algorithm in ALGORITHMS {
                    let input = Arc::clone(&bytes);
                    g.bench(format!("{algorithm:?}"), move |b| {
                        b.iter(|| black_box(hash(algorithm, black_box(&input), chunk)));
                    });
                }
            });
        }
    }
}

fn main() {
    validate();
    if std::env::args().nth(1).as_deref() == Some("--check") {
        println!("Known-answer and chunk-boundary checks passed for all benchmark adapters.");
        return;
    }
    let output = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: evif-hash-cost output.json"),
    );
    assert!(!output.exists(), "refusing to overwrite results");
    let result = zenbench::run(benchmarks);
    result.save(&output).expect("save complete samples");
}
