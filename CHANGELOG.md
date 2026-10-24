# Changelog

All notable changes to this repository. Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### QUEUED BREAKING CHANGES

- The format is draft v0: chunk layouts may change until the first tagged release.

## evif

### Added
- EVIF container: signature, `HEAD`/`BITS`/`PIXL`/`DIGS`/`clam`/`srce`/`knob`/`scor`/`SEAL`
  chunks, PNG-style tag case, XXH3-64 pixel digests over a domain-separated
  descriptor, whole-file `SEAL`. `no_std` + `alloc`, `forbid(unsafe_code)`.
- Tests: round trip, every truncation and single-bit flip refused, unknown ancillary
  chunks preserved, unknown critical chunks refused, digests cross-checked against an
  independent XXH3 implementation, strided rows.

## evif-probe

### Added
- `zenjpeg-expert` family over zenjpeg's `ExpertConfig` (per-component, per-band quant
  multipliers, zero-bias, subsampling, AQ, deringing, trellis) and the `corruption`
  family from corruption-corpus.
- Judges: zensim (codec-target profile), fast-ssim2, butteraugli (max norm, 3-norm),
  pinned to revisions recorded in every file.
- Matched-bitrate disagreement search: honest baseline curve per image, gains in
  baseline-span units, disagreement = target gain minus mean judge gain.
- `evif` CLI: `probe`, `inspect`, `verify`, `extract`, `list`; TSV ledger and
  `summary.md` per run.
