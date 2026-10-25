# EVIF, the evil image format [![CI](https://img.shields.io/github/actions/workflow/status/imazen/evif/ci.yml?style=flat-square&label=CI)](https://github.com/imazen/evif/actions/workflows/ci.yml) [![crates.io](https://img.shields.io/crates/v/evif?style=flat-square)](https://crates.io/crates/evif) [![lib.rs](https://img.shields.io/crates/v/evif?style=flat-square&label=lib.rs&color=blue)](https://lib.rs/crates/evif) [![docs.rs](https://img.shields.io/docsrs/evif?style=flat-square)](https://docs.rs/evif) [![license](https://img.shields.io/crates/l/evif?style=flat-square)](#license)

<p align="center"><img src="site/evif-banner.webp" width="960" alt="EVIF — the evil image format. A violet ghost-file with glowing green eyes floats over scattered pixels."></p>

**Find the images your quality metric gets wrong.**

EVIF is a container and a probe for stress-testing image quality metrics such as
SSIMULACRA2, butteraugli and zensim. An `.evif` file holds a distorted image made by
pushing a real encoder's knobs where no sane encoder goes, the exact recipe, one
lossless reference decode, and the score every metric gave it. The original
encoded bytes can be retained or omitted with their size and checksum recorded.
Anyone can re-run the metrics and check the recorded scores.

It is built for perceptual-metric research: finding the encodes a metric gets wrong,
so the metric can be fixed before an encoder is tuned against it.

## Why another IQA dataset

Fixed datasets (KADID-10k, TID2013, KADIS-700k) measure whether a metric ranks common
distortions the way people do. They say nothing about what an optimizer finds when it
pushes against the metric, and that is how metrics are actually used: inside encoder
tuning, rate-distortion search and codec pickers. A metric with 0.95 SROCC on KADID
can still have a cheap exploit, and an encoder tuned against it will find that exploit.

Maximum-differentiation competition (Wang & Simoncelli, 2008; gMAD, Ma et al., 2016)
answers this by holding one metric's score fixed and pushing another as far as it
will go, then asking a person which metric was right. EVIF runs that search over
codec parameters instead of pixel gradients, so every finding is a bitstream a real
decoder reads and a file size that is real. Pixel-space attacks produce noise no
encoder would emit; a quantization table a metric overrates is one an encoder can
actually ship.

## How the probe works

For each reference image and one *stress family* (a codec with its full parameter
vector exposed):

1. **Honest curve.** The family's default encoder is swept over quality to give a
   rate-distortion curve for every score key.
2. **Samples.** Parameter vectors are drawn at random: per-component, per-band
   quantization multipliers, zero-bias, chroma subsampling, adaptive quantization,
   deringing, trellis. Each is encoded, decoded by our decoder and scored by every
   judge.
3. **Equivalent bitrate.** Each key's score is read back through the honest curve
   into the bits per pixel an honest encode would need to score the same. The gain
   on a key is `log2(equivalent bpp / actual bpp)`: `+1` means the metric rates the
   encode like an honest file twice its size, `-1` like one half its size. Outside
   the swept range the equivalent clamps to the curve's ends, so a wrecked encode
   reads the same in every metric instead of being amplified by the metric with the
   narrowest scale.
4. **Disagreement.** The target metric's gain minus the mean of the others', in the
   same log2-bytes unit. The most disagreeing samples are written as `.evif` files,
   and every sample goes into a TSV ledger.

Disagreement is not ground truth: it says the metrics differ, not which one is wrong.
The written files are the pairs worth showing to people. Random search is a floor; a
proper optimizer over the same families comes next.

## Quick start

```sh
cargo build --release
target/release/evif list
target/release/evif probe --family zenjpeg-expert --target ssim2 \
    --out out/ssim2 --samples 256 --keep 8 corpus/*.png
target/release/evif inspect out/ssim2/<image>/zenjpeg-expert-017.evif
target/release/evif verify  out/ssim2/*/*.evif        # SEAL, PNG, pixel digest
target/release/evif extract out/ssim2/<image>/zenjpeg-expert-017.evif unpacked/
# Honest curve for an external scorer: decoded PNGs and render.tsv with bytes and bpp.
target/release/evif render --family zenjpeg-expert --out out/honest \
    --qualities 10,20,30,40,50,60,70,80,90 corpus/*.png
```

`inspect` prints what the file says about itself (a real file from a 24-sample run):

```
  1356x1132 Rgb U8
  bitstream: jpeg 29494 bytes
  interior bpp: 0.1537
  recipe: zenjpeg-color via zenjpeg 0.9.0 (a2a128ab)
  target: ssim2
  params: {"color":{"path":"ycbcr","subsampling":"s422"},"quality":36.77,
           "tables":{"family":"jpegli_shared","chroma_scales":[0.113,2.180]}, ...}
  zensim                    65.5594  (Higher, zensim@2b840717)
  ssim2                     62.1959  (Higher, fast-ssim2@54fabca9)
  butteraugli               18.2428  (Lower, butteraugli@02b570b4)
  butteraugli/3-norm         4.8084  (Lower, butteraugli@02b570b4)
  disagreement: 0.0797
  gain_vs_baseline: {"ssim2":-0.897,"zensim":-0.751,"butteraugli":-1.090, ...}
```

`extract` writes the retained candidate bytes, the stored PNG/IMIJ reference
decode, and scores as JSON. Colliding codec/rendition extensions use a separate
`bitstreams/` directory. `--format native` extracts only the original candidate;
it fails if those bytes were omitted. EVIF-specific candidate streams use the
`.evifbits` extension to distinguish them from complete `.evif` containers.

## What is in the coffin

| chunk | holds |
|---|---|
| `HEAD` | dimensions, channel layout, sample type |
| `BITS` | optional candidate bytes: an external codec or EVIF-specific compression |
| `BINF` | recorded candidate codec, encoded size and checksum when bytes are omitted |
| `PNGF` **or** `IMIJ` | one lossless reference decode: the exact pixels the metrics scored |
| `DIGS` | digests of the scored and source rasters |
| `clam` | explicitly synthetic claimed cost, when no encoded cost exists |
| `srce` `knob` `scor` | source, recipe and versioned scores |
| `SEAL` | checksum of the preceding file bytes |

PNG and IMIJ are alternatives; a file stores exactly one. Viewers show
that reference decode because candidate decoders can differ in IDCT, chroma
upsampling or film grain. EVIF-specific compression goes in `BITS`, not in the
reference decode. Legacy `PIXL` files remain readable. See [SPEC.md](SPEC.md).

### Storage and output choices

```sh
# One PNG rendition per candidate, without a second file of original codec bytes.
evif render --family zenjpeg-expert --qualities 20,60,90 --out out/png \
    --format png --omit-bits input.png
# Complete containers with PNGF reference decodes and recorded candidate costs.
evif render --family zenjpeg-expert --qualities 20,60,90 --out out/evif \
    --format evif --rendition png --omit-bits input.png
# Remove BITS from an existing file while preserving recipe, scores and provenance.
evif repack input.evif compact.evif --omit-bits
# Export only the retained original codec file.
evif extract input.evif out/native --format native
```

`render --format` accepts `png`, `imij`, `native`, or `evif`. `probe` and EVIF
rendering accept `--rendition png|imij` and `--omit-bits`. The public binary
provides PNG; a companion registers the IMIJ encoder/decoder through
`Registry::add_rendition_codec`. The container library can preserve and extract
IMIJ bytes without that plugin. Native output needs actual candidate bytes.

Existing defaults retain candidate bytes and use PNG reference decodes.
`render.tsv` keeps its original first six columns and appends output format/path,
candidate codec/checksum, and retained bitstream path. An empty `png` column
means the selected output is not PNG. Candidate bpp counts the candidate codec,
never PNG/IMIJ storage or container overhead. `inspect` distinguishes embedded,
recorded, synthetic claimed, and unknown costs. Recorded checksums cannot prove
that an omitted bitstream can still be reproduced.

For Rust callers migrating from draft 0.0, `Evif.png` becomes
`Evif.rendition.data`; check `Evif.rendition.format` before decoding it.
`Evif::new` still accepts PNG bytes. Handle `Bpp::Recorded` when matching costs.

## HDR and video design

The current CLI evaluates RGB8 still images. The [HDR and sequence proposal](docs/hdr-video-evaluations.md)
separates lossless rendition storage, exact timing, viewing conditions and metric
execution. It includes executable contract and carrier checks; end-to-end HDR/video
support is not yet implemented.

## Families and judges

| family | what it varies |
|---|---|
| `zenjpeg-expert` | zenjpeg's `ExpertConfig`: 3×4 quant multipliers (component × DC/low/mid/high band), zero-bias, 4:4:4/4:2:2/4:4:0/4:2:0, AQ, deringing, trellis |
| `zenjpeg-color` | zenjpeg's color pipeline: YCbCr / XYB / RGB passthrough, jpegli chroma distance scales or mozjpeg chroma quality, gamma-aware chroma downsampling, deringing, AQ, auto-optimize |
| `zenpng-palette` | lossy PNG through zenpng + zenquant: palette size 2–256, quality preset, six dither modes and strength, Viterbi run weight |
| `corruption` | the structural defects of [corruption-corpus](https://github.com/imazen/codec-corpus): channel swaps, dropped blocks, off-by-one edges, wrong-phase chroma, real decoder bugs; pixels only, no bitstream |

| judge | keys |
|---|---|
| zensim | `zensim` (current codec-target profile) |
| fast-ssim2 | `ssim2` |
| butteraugli | `butteraugli` (max norm), `butteraugli/3-norm` |

Judges are pinned to specific revisions ([`evif-probe/src/pins.rs`](evif-probe/src/pins.rs)),
and every score row names the implementation that produced it.

Families and judges are plug-ins: `evif_probe::registry::Registry` holds the built-in
set, and a companion binary can add its own and reuse the CLI (`evif_probe::cli::main`).
That is how families and judges built on unpublished code join the same runs.

Coming: zenwebp, zenjxl and zenavif families; a post-filter family with no rate
constraint (sharpening, grain, hue, sub-pixel shift); a human-judgment loop.

## Crates

- **`evif`**: the container. `no_std` + `alloc`, `#![forbid(unsafe_code)]`, reads and
  writes files and computes digests; decodes no image codec. MIT OR Apache-2.0.
- **`evif-probe`**: families, judges, the search and the `evif` CLI. Links the zen
  codecs and metrics at pinned revisions. AGPL-3.0 OR commercial, not published.

## License

`evif` is dual-licensed under MIT and Apache-2.0. `evif-probe` is AGPL-3.0-only or
the Imazen commercial license (see `evif-probe/LICENSE-*`).
