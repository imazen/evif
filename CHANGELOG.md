# Changelog

All notable changes to this repository. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### QUEUED BREAKING CHANGES

- The format is draft v0: chunk layouts may change until the first tagged release.
- Draft 0.1 replaces `Evif.png` with `Evif.rendition`, adds `Bpp::Recorded`,
  and writes PNGF/IMIJ instead of PIXL. Legacy PIXL files remain readable;
  old readers refuse the new critical chunks (`a6f005be`).

## Workspace

### Changed
- Website and README use haunted-file artwork, with a responsive violet and lime
  theme and matching SVG icons (`486fee4e`).

## evif

### Added
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

### Changed
- Reference codec registration supports companion IMIJ implementations. Storage
  flags select PNG/IMIJ, omit candidate bytes, repack containers, or export
  PNG/IMIJ/native/EVIF while preserving original candidate cost. Render ledgers
  append explicit format, path and provenance columns (`b2597dcc`).

### Fixed
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
