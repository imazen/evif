# Codec possession

Build the opt-in families with Rust 1.89 or later:

```sh
cargo build --release -p evif-probe --features possession
target/release/evif list
target/release/evif probe --family zenav1-svt --target ssim2 \
    --samples 256 --keep 8 --out out/svt corpus/*.png
target/release/evif probe --family zenjxl --target butteraugli \
    --samples 256 --keep 8 --out out/jxl corpus/*.png
target/release/evif render --family zenjpegai --out out/jpegai \
    --qualities 10,30,50,70,90 --format evif corpus/*.png
target/release/evif render --family zencoolchic --out out/coolchic \
    --qualities 10,30,50,70,90 --format evif corpus/*.png
```

Set `ZENJPEGAI_MODELS` to the JPEG AI reference checkpoint directory before
using `zenjpegai`; missing models are a hard encode error. Enable only one
codec with `possession-svt`, `possession-jxl`, `possession-jpegai` or
`possession-coolchic`. The container crate and default probe build do not
acquire these codec dependencies. The IMIJ-enabled companion forwards the
same features and can use `--rendition imij --omit-bits`.

These families take opaque sRGB RGB8 stills, write a real codec file, then decode
that file to obtain the pixels scored and stored in the rendition. Encoded cost
includes the complete AVIF file or JXL codestream. This is not an HDR/video probe.

| Family | Exposed controls | Baseline |
|---|---|---|
| `zenav1-svt` | quality, speed, tune, variance boost/strength/octile, QM and levels, sharpness, screen-content detection, AC bias, maximum transform size, independent U/V quantizer deltas | zenavif SVT backend, speed 6, PSNR tune, 8-bit 4:2:0, one thread, default expert settings |
| `zenjxl` | native distance, effort, gaborish, EPF, resampling, noise synthesis, faster decoding, initial AC quantization, transform information-loss weight, CfL two-pass, chromacity adjustment, small transforms | native quality-to-distance mapping, effort 7, one thread, default filters/quantization |
| `zenjpegai` | trained model ID, quantizer displacement, residual-variance scaling, channel gain, latent scaling | model 1, no extra tools, rate displacement spanning low to high bitrates |
| `zencoolchic` | native lambda, iteration budget, coded architecture, seed, RDOQ | 2000 training iterations, high architecture, seed 0, RDOQ on |

JPEG AI is stored with the `jaic` bitstream FourCC, Cool-chic with `cool`.
These identify the encoded syntax; the recipe pins the encoder implementation.
JPEG AI model weights are separate data and must be retained with their own
provenance for a fully reproducible run. Its quality-to-displacement mapping is
uncalibrated. Cool-chic trains a model per image; 2000 iterations is its minimum
and a sample can take substantial time. Its quality-to-lambda mapping is also
uncalibrated. Run a small render
before a broad search and keep the honest baseline from the same family.

The JXL baseline clamps quality to 1–99 to stay on the lossy path. SVT accepts
1–100. Neither claims that a quality number means the same thing across codecs.
The honest curve belongs to the same family as the candidate.

These are selected controls, not the complete encoder parameter spaces. SVT is
4:2:0 only here. Film-grain controls, AV1 tiling, JXL modular/lossless controls,
HDR samples and video are not exposed by these families.

## Search priorities and replay

Each draw has an 80% chance of selecting one likely stress axis: SVT chroma
quantization, QM, variance boost or sharpness/AC bias; JXL quantization, filters,
resampling or chroma prediction. The other 20% combines controls, including speed,
tune, transform restrictions, noise and decoding effort. Both portions sample
quality uniformly, including aggressive compression. These weights are a search
policy, not measured probabilities of finding a metric failure.

SVT tune 3/4 override some QM, sharpness and variance settings inside the codec.
The recipe records requested settings and exact implementation revisions; it does
not claim each setting is independent under every tune. JXL effort also gates
some tools. Unknown parameter names and values outside the exposed ranges are
rejected before encoding. The implementation identifier binds wrapper, encoder
and decoder revisions and is checked on replay.

The SVT seam pins zenav1-svt `0f63ca13757eea3075f29656b7886143073d2022`;
zenavif is `a7638f6e0655914fb15fe4633ac26425533756c8` and rav1d-safe is
`dfd01e3268f6e8da862d2d5f0a71d19f414f2b87`. JXL uses zenjxl
`3f5e22e9cf8e2d58311af183d3f20454deeae6be`, jxl-encoder
`ce4ec7a8e4bf4a74a0b55a1851bc5b2b84167640`, and zenjxl-decoder
`21159c6ba149bc802f0d985851e568976a44da04`. Root Cargo patches supply the
unreleased JXL dependencies. Keep these pins, recipe identifiers and Cargo.lock
in step when upgrading.

Run `just possession-check` and `just possession-test`. Set `HEAVY` to the
resource wrapper for the machine. `EVIF_POSSESSION_ARTIFACTS` optionally exports
native baseline bytes, the decoded PNG rendition and JSON parameters for an
independent decoder check; it does not alter which tests run.

[Integration checks and retained artifacts](../benchmarks/possession-2026-10-26.pointer.md).
