# Changelog

All notable changes to this repository. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### QUEUED BREAKING CHANGES

- Major-1 hashes now use `{algorithm, digest}` records with variable-length hex.
  Samples use `canonical` plus optional `checks`; payloads and MEND use `hash`,
  and score references carry tagged hashes. Media domain revision 4 binds the
  canonical algorithm, digest length and bytes. Unknown sample algorithms are
  preserved; unsupported payload/root verification returns an error (`6fb85af4`).
- Canonical major-1 sample hashes now use XXH3-128 through zenhash.
  `SampleHash` wraps `u128`; `SampleHashes.xxh3_128` serializes as `xxh3-128`
  with 32 lowercase hex digits. Media roots use domain revision 3 and bind all
  16 digest bytes. Earlier draft files require rehashing their decoded samples
  (`b76c35c2`). [Verification](benchmarks/wire-xxh128-2026-10-25.pointer.md).

- The earlier major-1 draft introduced canonical XXH3-64 sample hashes with optional
  SHA-256 checks. `Frame.samples` and `frame_hash` use `SampleHashes`; that draft used media
  domain revision 2 and exclude optional checks. Earlier draft media must be
  regenerated from decoded samples (`3b1746ed`).
- Opt-in wire major 1 has new framing and media semantics; major-0 readers refuse
  it. The legacy Rust API and probe commands retain their format (`f12f8cca`).
- The format is draft v0: chunk layouts may change until the first tagged release.
- Draft 0.1 replaces `Evif.png` with `Evif.rendition`, adds `Bpp::Recorded`,
  and writes PNGF/IMIJ instead of PIXL. Legacy PIXL files remain readable;
  old readers refuse the new critical chunks (`a6f005be`).

## Workspace

### Added
- Major-1 IMIJ page windows and backward carrier sharing for lossless
  reference-predicted renditions; selected samples retain their media identities
  (`d748464d`).
- Opt-in zenav1-svt and JPEG XL possession families with pinned codec replay,
  decoded candidate renditions, prioritized stress axes and a combination tail
  (`95cc0f37`).
- Isolated Rust hash comparison tool, pinned implementations, streaming controls
  and measured single-thread costs; no wire algorithm change (`2e6ecfbe`).
- Resolved SDR/HDR rendition design profile with exact frame timing, explicit
  signal units, and segment-plan checks. Gain maps and paired SDR/HDR copies
  are not required. This does not enable container video I/O (`9618f932`).
- HDR/sequence evaluation proposal with exact timing, condition bindings and
  storage-independent evaluation identities; executable design checks (`4e6d5471`).
- PQ/HLG APNG carrier tests and an independent sample read-back oracle (`26408c0b`,
  `2f177da1`). These do not enable EVIF video output.

### Changed
- Website and README use haunted-file artwork, with a responsive violet and lime
  theme and matching SVG icons (`486fee4e`).

## evif

### Added
- Major-1 symmetric still/video assets, u64 record lengths, exact rational timing,
  SDR/PQ/HLG/linear-float profiles, segmented PNGF/IMIJ, optional BITS, SHA-256
  sample/media identities, and appendable scoring envelopes. Includes bounded
  streaming I/O and a no_std slice reader (`f12f8cca`, `0f81010f`).
- Exactly one PNGF or IMIJ reference decode; optional BITS with recorded codec,
  size and checksum in BINF. EVIF-specific candidate coding belongs in BITS
  (`a6f005be`).
- EVIF container: signature, `HEAD`/`BITS`/`PIXL`/`DIGS`/`clam`/`srce`/`knob`/`scor`/`SEAL`
  chunks, PNG-style tag case, XXH3-64 pixel digests over a domain-separated
  descriptor, whole-file `SEAL`. `no_std` + `alloc`, `forbid(unsafe_code)`.
- Tests: round trip, every truncation and single-bit flip refused, unknown ancillary
  chunks preserved, unknown critical chunks refused, digests cross-checked against an
  independent XXH3 implementation, strided rows.

## evif-probe

### Added
- Opt-in JPEG AI and Cool-chic possession families, with native-byte decoding
  and a documented one-call external codec registry hook (`97c07ad5`).
- `evif wire pack`, `inspect`, and `append`; score JSON passes through byte for
  byte and existing outputs are not overwritten (`0f81010f`).
- Real PQ/HLG APNG wire fixtures with symmetric assets and an independent Python
  framing/hash reader plus FFmpeg sample verification (`0d6cdfc7`).

### Changed
- Reference codec registration supports companion IMIJ implementations. Storage
  flags select PNG/IMIJ, omit candidate bytes, repack containers, or export
  PNG/IMIJ/native/EVIF while preserving original candidate cost. Render ledgers
  append explicit format, path and provenance columns (`b2597dcc`).

### Fixed
- Extraction refuses existing score files; render rejects legacy ledgers before
  appending incompatible columns (`57ba3891`).
- PNG extraction preserves the interior bitstream under `bitstreams/` alongside
  the scored rendition (`2b81191d`).

### Added
- `evif render` also preserves original encoded bytes under `bitstreams/`,
  using the rendition stem and the codec extension (`f269a3f9`).
- `zenjpeg-expert` family over zenjpeg's `ExpertConfig` (per-component, per-band quant
  multipliers, zero-bias, subsampling, AQ, deringing, trellis) and the `corruption`
  family from corruption-corpus.
- `zenjpeg-color` family (YCbCr/XYB/RGB paths, jpegli chroma distance scales, mozjpeg
  chroma quality, gamma-aware downsampling) and `zenpng-palette` (zenpng indexed output
  from zenquant: palette size, preset, dither mode and strength, Viterbi weight).
- `Registry` of families and judges, and `cli::main(&Registry)`, so a companion crate
  can add families and judges built on unpublished code and reuse the CLI.
- The search skips parameter vectors a family refuses and reports them.
- butteraugli is linked without `rayon`, so no thread pool takes part in a score.
- The container parses JSON floats exactly (`serde_json/float_roundtrip`): the default
  parser returned a value one ulp off, and re-scoring a file then failed to match.
- Judges: zensim (codec-target profile), fast-ssim2, butteraugli (max norm, 3-norm),
  pinned to revisions recorded in every file.
- Matched-bitrate disagreement search: honest baseline curve per image; each score is
  mapped to the bpp an honest encode would need for it, gain = log2(equivalent bpp /
  actual bpp), disagreement = target gain minus mean judge gain.
- `evif` CLI: `probe`, `inspect`, `verify`, `extract`, `list`; TSV ledger and
  `summary.md` per run.
- `evif render`: encode images with a family at baseline qualities or explicit parameter
  vectors and write the decoded PNGs plus a TSV of sizes, so external scorers can build
  the honest rate-distortion curve or re-render a recorded recipe.
