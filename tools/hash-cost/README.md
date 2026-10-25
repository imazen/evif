# Hash cost experiment

Research tool only. It does not change EVIF's wire algorithms or add dependencies
to the shipped crates. The lockfile pins implementation versions. Libraries are
used under their own licenses, without vendoring or modifying their algorithms.

[Measured costs and selection notes](../../benchmarks/hash-cost-2026-10-25.md)
include the CPU, implementation caveats, all cases and retained raw results.
The [XXH3 width comparison](../../benchmarks/xxh3-widths-2026-10-25.md) measures
64/128-bit costs and separates output stability from statistical-test results.
The [WASM and quality investigation](../../benchmarks/hash-portability-2026-10-25.md)
records two-engine measurements, unsafe-code audits and a reproducible
fixed-secret Rapidhash collision control (`just hash-quality-controls`).

```
just hash-cost-build
just hash-cost-check
just hash-cost /absolute/path/to/new-results.json
just hash-cost-xxh3 /absolute/path/to/width-results.json
```

Set `TMPDIR` to persistent scratch and `HEAVY` to the machine's resource wrapper.
Save stdout/stderr to a log alongside the results. The output path must not exist.
Record source commit, CPU, compiler and command when publishing results.

The nine arms use incremental APIs: XXH3-64/128 (zenhash), portable Rapidhash v3
with default C-compatible secrets, MuseAir v2 Standard-128 with zero seeds,
SHA-256, BLAKE3's ordinary unkeyed mode, KT128, KT256, and NIST Ascon-Hash256.
Both KT functions use empty customization and produce 32 bytes. All crypto arms
produce 32 bytes; noncrypto arms retain their native 8/16-byte digest widths.

Each timed call creates a hasher, consumes an already allocated deterministic
byte pattern, and finalizes. There is no allocation, decoding, file I/O, padding
removal or format metadata inside the timed call. Cases are 64 B, 4 KiB, 1 MiB,
and 64 MiB. Large inputs are measured both as a single update and as 16 KiB
updates. These model update granularity, not an actual strided image or a full
decoder pipeline. Known-answer examples and chunk-boundary equivalence checks
run before timing. `--check` runs these controls without benchmarking.
The `hash-cost-xxh3` recipe selects only the two XXH3 widths, with identical
streaming updates and different finalization. Its 128-bit constants were also
checked against the independent C xxHash 0.8.3 library.

Zenbench interleaves 30 rounds per group. Input is reused; these are warm/repeated
buffer measurements, not cold-file or storage throughput. There is one hashing
thread, no Rayon, no `target-cpu=native`, and default crate CPU dispatch. RustCrypto
`k12` 0.5.1 with `keccak` 0.2.2 selects its software backend on x86 by default;
this is not a benchmark of K12's optimized SIMD ceiling. BLAKE3 and SHA-256 can
use their default hardware acceleration. Report implementation costs accordingly.

## WASM comparison

```
just hash-cost-wasm-build
just hash-cost-wasm /absolute/artifact-directory wasmtime.json
just hash-cost-wasm-v8 /absolute/artifact-directory v8.json
```

These recipes select XXH3-64/128, portable Rapidhash V3, MuseAir Standard-128
and BLAKE3. The module explicitly requires SIMD128, including BLAKE3's
`wasm32_simd` feature. Both engines run the same module and time the same Rust
streaming calls; module loading/compilation is outside those measurements.
WASI cannot inspect host load, so the guest resource gate is explicitly disabled.
Use the host resource wrapper and record its results. Record the module hash,
source commit, engine version and host CPU separately from guest metadata.

`build-wasi.py` prepares a separate copy of pinned zenbench 0.1.10 under
`TMPDIR`. It replaces unsupported WASI scratch/PID discovery and disables the
guest cross-process lock. It does not change timing or statistics. Original
dependency sources and the native lockfile are preserved. This research-only
compatibility patch avoids changing the measurement engine to a newer release.
