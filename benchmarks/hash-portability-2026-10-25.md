# Hash quality and WASM portability

These are research results, not a change to EVIF's wire hash. They extend the
[native comparison](hash-cost-2026-10-25.md) and
[XXH3 width comparison](xxh3-widths-2026-10-25.md). Published SMHasher3 results
and locally reproduced controls are identified separately.

Decision, October 25, 2026: use our zenhash implementation. EVIF already depends
on it. The canonical wire hash remains XXH3-64 with the standard default secret
and zero seed; optional SHA-256 sample checks remain available. Selecting the
implementation does not select a different digest width or change the wire
contract. The measured portability and existing implementation support this
choice; the statistical limitations documented below still apply.

## What the XXH3 failures mean

The published [XXH3-64 report](https://gitlab.com/fwojcik/smhasher3/-/raw/main/results/raw/XXH3-64.txt)
passes 223/250 cases; the [XXH3-128 report](https://gitlab.com/fwojcik/smhasher3/-/raw/main/results/raw/XXH3-128.txt)
passes 214/250. These counts are neither collision probabilities nor a quality
ranking. Concrete examples from those reports:

- Bit independence: the three-byte case reports maximum correlation 1.0000 in
  both widths. Output-bit changes are not independent in this test. Other short
  lengths also fail. This is a published observation, not a local reproduction.
- Sparse three-byte inputs: the 128-bit report observes 470 collisions versus
  128 expected in the high 40 bits of differential outputs. Full-width ordinary
  hash collisions in this case are zero. `Sparse 20/3` denotes up to 20 set bits
  in three-byte inputs, not 20-byte inputs.
- SeedZeroes, lengths up to 1280 bytes: 4160 sparse seeds produce 5,324,800
  hashes. XXH3-64 has 571 full-width collisions. XXH3-128 has zero full-width
  collisions in this case, but its high/low halves have 571/569 collisions.
- SeedBlockLen, eight-byte inputs: XXH3-128 has **534,343 full-width collisions
  among 2,196,480 hashes**, with effectively zero expected. Seeds and four-byte
  blocks each have up to two set bits; block offsets are zero and four.

The seed tests vary both inputs and seeds. Their collision counts must not be
presented as rates for EVIF's fixed default-seed image hashes. Conversely, these
are not all small statistical biases: some cases contain actual full-width
collisions. Moving to 128 bits does not remove the entire failure family.

## Rapidhash variants and a separate collision control

Published reports show 250/250 passes for
[V3](https://gitlab.com/fwojcik/smhasher3/-/raw/main/results/raw/rapidhash.txt),
[protected V3](https://gitlab.com/fwojcik/smhasher3/-/raw/main/results/raw/rapidhash.protected.txt),
[micro](https://gitlab.com/fwojcik/smhasher3/-/raw/main/results/raw/rapidhash-micro.txt),
[nano](https://gitlab.com/fwojcik/smhasher3/-/raw/main/results/raw/rapidhash-nano.txt), and
[Rust quality::RapidHasher](https://gitlab.com/fwojcik/smhasher3/-/raw/main/results/raw/rust-rapidhash.txt).
The separately registered [Rust fast::RapidHasher](https://gitlab.com/fwojcik/smhasher3/-/raw/main/results/raw/rust-rapidhash.fast.txt)
passes 118/250, failing 132 cases. Rust's `fast` API is not the same naming choice
as C's `RAPIDHASH_FAST` multiplication mode.

The [Rust crate's versioning contract](https://docs.rs/rapidhash/4.5.1/rapidhash/)
distinguishes stable, portable V1/V2/V3 byte functions from the `fast` and
`quality` in-memory APIs, whose outputs may change. A format would need to pin
the algorithm version, secrets, mode, finalization and byte serialization.

Passing the suite does not establish resistance to deliberately chosen inputs.
The Rust author's [fixed-secret collision analysis](https://liams.website/articles/seed-independent-collisions-on-wyhash-and-rapidhash)
describes a multiplication operand becoming zero and erasing earlier state.
We reproduced this with rapidhash 4.5.1's standard `RapidStreamHasherV3` and its
default public secrets. Sixteen distinct inputs at each of 32 bytes and 1 MiB
all returned `ffffffffffffffff`. Setting the penultimate eight bytes to
`secret[1] XOR tail_length` triggers the construction.

The same inputs produced 16 distinct digests each with protected V3
(`RapidStreamHasherInlineV3<true, true>`), MuseAir Standard-128 and XXH3-128.
That is a control for this construction, not a security proof for those hashes.
Random secrets can serve hash-table defenses, but public format parameters
cannot be treated as hidden secrets.

Reproduction: `just hash-quality-controls`, source commit
`eedd0bcd368ea232cc8fb6ee5c3d9f478bd88f75`. Formatting, release Clippy and the
benchmark adapter known-answer/chunk-boundary checks passed.

## WASM SIMD128 measurements

Same module and workloads in both engines: Ryzen 9 5900XT, one thread pinned
to CPU 2, Rust 1.99.1 / LLVM 23.1.1, thin LTO, `wasm32-wasip1`, explicit
`-C target-feature=+simd128` and BLAKE3's `wasm32_simd` feature. No
`target-cpu=native`, relaxed SIMD or wide-arithmetic extension was enabled.
These are WASI executions on an x86 host, not browser or ARM measurements.

The table is GiB/s for a reused 64 MiB buffer, streamed in 16 KiB updates.
Initialization and finalization are included; module loading and compilation
are outside timing. Thirty interleaved rounds; all table cells have CV below
1.9%. The [CSV](hash-portability-2026-10-25.csv) includes every workload.

| Hash / implementation | Wasmtime 40.0.1 | Node 26.7.0 / V8 14.6 |
|---|---:|---:|
| XXH3-64 / zenhash | 12.491 | 12.518 |
| XXH3-128 / zenhash | 12.524 | 12.506 |
| Rapidhash V3 standard / 4.5.1 | 3.037 | 4.419 |
| MuseAir v2 Standard-128 / 0.6.0 | 2.853 | 4.207 |
| BLAKE3 / 1.8.7 | 1.959 | 1.660 |

For 64-byte inputs, XXH3-128 took 67.604 ns in Wasmtime and 35.573 ns in V8.
The native measurements are linked above; no ARM execution was measured.
`cargo check --release --locked --target aarch64-unknown-linux-gnu` passed for
the research crate with its pinned dependencies, using the AArch64 C compiler
for dependencies. This is a cross-compilation check, not linked/run validation.

The implementation explains an important portability tradeoff: XXH3's WASM
accumulation uses vector 32-bit widening multiplication. Rapidhash and MuseAir
use scalar 64-by-64-to-128-bit multiplication. Baseline SIMD128 does not provide
that full product as one operation. Their native x86/AArch64 strengths therefore
do not establish equal WASM performance. This is a source-based explanation,
not a measured instruction-level attribution or an ARM speed claim.

## Unsafe audit and selection implications

- Our pinned zenhash source forbids unsafe code and has scalar, x86, NEON and
  WASM SIMD128 paths. Its safe SIMD interface uses archmage. A crate-level
  prohibition does not prohibit unsafe internals in dependencies.
- [MuseAir 0.6.0](https://docs.rs/museair/0.6.0/museair/) already has
  `#![forbid(unsafe_code)]` and no dependencies. Standard-128 is the candidate
  measured here; do not substitute BFast and assume identical quality.
- Rapidhash 4.5.1 has unconditional unsafe unaligned reads in `src/util/read.rs`
  and unsafe assumptions in `src/v3/rapid_const.rs`. Disabling its `unsafe`
  feature restores certain assertions; it does not eliminate unsafe code.
  A safe port of its byte API is feasible, but was not implemented or timed.
- BLAKE3 1.8.7's optimized implementation contains unsafe code. Its `pure`
  feature removes C/assembly implementations, not all unsafe Rust. An optimized
  implementation through our safe SIMD abstractions would be additional work.

XXH3-128 offers established output stability, existing owned implementation and
the best measured WASM throughput here, but fails the strict all-SMHasher3-tests
requirement. MuseAir Standard-128 fits that statistical requirement and the
unsafe-code policy at a measured WASM cost. Standard Rapidhash V3's public-secret
collision construction makes it unattractive as persistent sample identity.
Protected V3 avoids this specific control, but has not been speed-measured here.
BLAKE3 serves the separate requirement for cryptographic collision resistance.
No hash in this comparison dominates every requirement.

## Reproduction and retained evidence

WASM source commit: `644caa51895a73786def88ea8a55fde79d401cd2`.
Pinned dependency versions are in `tools/hash-cost/Cargo.lock`. The builder
copies pinned zenbench 0.1.10 and patches only unsupported WASI scratch paths,
PID-derived run IDs and cross-process locking. Timing/statistics are unchanged.
The guest resource gate is disabled explicitly; the host wrapper stays active.

```sh
export TMPDIR="$HOME/tmp"
export HEAVY="$HOME/work/claudehints/scripts/run-heavy --mem 12G --jobs 4"
just hash-cost-wasm-build
export HEAVY="$HEAVY -- taskset -c 2"
just hash-cost-wasm /mnt/v/output/evif/hash-wasm-20261025 wasmtime.json
just hash-cost-wasm-v8 /mnt/v/output/evif/hash-wasm-20261025 v8.json
```

Use fresh output names when rerunning; results are not overwritten.
The module SHA-256 is
`122bcc272b708b18cc31fe3a6c784d7c6393012fa1388581f3c69988c74d5251`.
Raw JSON, logs, module, published report captures and their URLs are retained
at `/mnt/v/output/evif/hash-wasm-20261025/` on r5900xt, with `provenance.json`,
`sources.json` and `sha256.json`.

Resource-wrapper measurements, including the engine process:

```text
Wasmtime: run-heavy: done rc=0 10s | peak-RSS 0.14GiB | min-avail 50306MiB | peak-load 0.44
V8:       run-heavy: done rc=0 10s | peak-RSS 0.13GiB | min-avail 50447MiB | peak-load 0.37
Control:  run-heavy: done rc=0 2s | peak-RSS 0.20GiB | min-avail 50547MiB | peak-load 0.12
ARM check: run-heavy: done rc=0 2s | peak-RSS 0.25GiB | min-avail 50172MiB | peak-load 0.54
```
