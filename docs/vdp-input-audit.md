# CVVDP and HDR-VDP input audit

2026-10-25. Source/API audit, not an end-to-end scoring run.

**A current `.evif` cannot supply an HDR sequence to these metrics.** The
[sequence proposal](hdr-video-evaluations.md) has the right separation of media,
presentation and evaluation, but needs the explicit bindings below before it can
claim sufficient data. PNG/APNG and IMI can carry the usual RGB samples; the
missing pieces are contracts and adapters, not a requirement for original codec
bytes or duplicate PNG and IMI renditions.

## Revisions inspected

- zenmetrics remote branches and current `master` at
  [`d7cced0fc4648710255c487700712c7c0c40b828`](https://github.com/imazen/zenmetrics/tree/d7cced0fc4648710255c487700712c7c0c40b828).
  The latest commits touching the three crates among the fetched remote branches
  are included in this master: CPU CVVDP `88ec8076`, GPU CVVDP `abb62630`, HDR-VDP
  `4f89155c`. The CPU and GPU CVVDP reference constant is `v0.5.7`.
- Official ColorVideoVDP main at
  [`166f5ae95ecbd841ae727984cd237de213875399`](https://github.com/gfxdisp/ColorVideoVDP/tree/166f5ae95ecbd841ae727984cd237de213875399).
  Its base parameter file still reports `0.5.7`; that label alone does not pin
  all implementation behavior or configuration files.
- Official [HDR-VDP 2.2.2](https://sourceforge.net/projects/hdrvdp/files/hdrvdp/2.2.2/)
  and [HDR-VDP 3.0.7](https://sourceforge.net/projects/hdrvdp/files/hdrvdp/3.0.7/)
  release archives, including `hdrvdp.m`, `hdrvdp3.m`, option parsing, and the v3
  video example. SHA-256 of downloaded ZIPs, respectively:
  `b651bc474a47050b8c107484c0aff8571f5b237264eaa6d898f85a2ae7766820` and
  `baa5c25cca2e38176345a2a5423793459fbf571282396640c5e4ac85bf8381b3`.

## Current EVIF versus the proposal

The current [HEAD](../SPEC.md#2-head) defines only sRGB, u8/u16 and one raster.
The rendition payload is opaque to the container; putting an APNG inside PNGF
does not give HEAD, DIGS, the reader or the scorer sequence semantics. The
[judge](../evif-probe/src/judge.rs) takes two `Rgb8Image` values. No critical
presentation record or temporal adapter is implemented.

The stored reference decode is the **candidate decoder's output**. Full-reference
metrics also need the pristine source sequence. A self-contained evaluation must
embed both logical assets; a collection may instead resolve the source by content
identity. A source name, URL or digest without available source samples is not
sufficient. Original candidate BITS can remain omitted in either mode.

| Requirement | Proposal coverage | Work needed to make it enforceable |
|---|---|---|
| Both input sequences and exact sample identities | Explicit assets/dependencies | Reader, source resolution, complete frame coverage |
| Sample precision, color, units, range, alpha | Explicit descriptors and preparation | Native-depth adapters; deterministic conversion and alpha composition |
| Exact timeline and experiment interval | Rational timing and explicit alignment | CFR delivery map for CVVDP; sampled-frame support for HDR-VDP |
| Display photometry and geometry | Broad presentation record | Per-metric resolved field bindings, including exposure and reflected light |
| Observer and surround | Mentioned generically | Age, typed surround modes and channel values, explicit optics configuration |
| Spectral emission | Not specified concretely | Preset plus table identity, or wavelength/channel table dependency |
| Metric identity and outputs | Versioned configuration/results | Task and output keys, native versus wrapper pooling, unsupported-feature refusal |

## CVVDP

### Official implementation

[`video_source`](https://github.com/gfxdisp/ColorVideoVDP/blob/166f5ae95ecbd841ae727984cd237de213875399/pycvvdp/video_source.py)
supplies paired frames, dimensions, frame count, and one frame rate. The array
adapter normalizes u8 by 255 and u16 by 65535, accepts f32, and widens f16 to f32.
It admits one or three channels; alpha needs prior composition. The metric's
one-frame path is a still, while multiple frames use temporal filters. Native
per-frame timestamps are not part of this interface: VFR needs an explicit
uniform-rate derivative, with the delivery rate and source-to-delivery map saved.

[`display_model.py`](https://github.com/gfxdisp/ColorVideoVDP/blob/166f5ae95ecbd841ae727984cd237de213875399/pycvvdp/display_model.py)
uses source color space/matrix, EOTF, peak luminance, contrast or intrinsic black,
ambient illuminance, reflectivity, and exposure. Geometry may be supplied through
physical dimensions/distance, field of view, or ppd. Preserve the actual resolved
values and configuration-file identities. The prepared frame's dimensions and
displayed image geometry must describe the same experiment.

### Rust CPU port and generic adapter

The [direct video API](https://github.com/imazen/zenmetrics/blob/d7cced0f/crates/cvvdp/src/video.rs)
already has `push_frame`, `push_frame_u16`, and `push_frame_f32`. It takes RGB,
fixed dimensions, a positive f32 rate, display parameters, physical geometry, and
options for layout, padding, temporal filter, feature resampling and heatmaps.
The checked constructor requires dimensions at least 8 by 8. It accepts one
sample type per scorer. Frames are contiguous interleaved or planar RGB;
an EVIF adapter must pack strided rows correctly. F16 can widen to f32 without
losing finite sample precision.

For relative EOTFs, f32 means normalized code values; for `Eotf::Linear`, it
means absolute cd/m². U16 always divides by 65535. A 10-bit value in a u16 word
therefore needs an explicit alignment/range/normalization recipe: merely naming
the storage type is insufficient. Preserve rational timing in EVIF and record
the effective f32 rate delivered to this API.

The [generic `Metric` implementation](https://github.com/imazen/zenmetrics/blob/d7cced0f/crates/cvvdp/src/pipeline.rs#L4372)
accepts only RGB8 sRGB. Its sequence wrapper uses that same RGB8 ingress and the
default direct-video options; map requests are refused there. Reusing the shared
abstractions does **not** automatically expose native-depth video, selectable
temporal settings or video heatmaps. Those need an adapter extension or an
internal bridge to the existing direct API.

The Rust [display model](https://github.com/imazen/zenmetrics/blob/d7cced0f/crates/cvvdp/src/params.rs)
has peak, intrinsic black, reflected light, EOTF, primaries, ambient lux and
reflectivity, but no exposure field. Its video geometry is the physical
resolution/distance/diagonal structure. A ppd-only experimental preset needs a
declared conversion or a direct-ppd API extension; never invent a measured
physical display. Non-unit exposure needs a verified preparation path or refusal.
Several algorithm parameter bundles in `CvvdpParams` are scaffolding rather than
runtime-loaded model overrides; an adapter must reject unsupported overrides.

The Rust GPU crate exposes the still pipeline; its source has no equivalent
video scorer. CPU temporal support must not be advertised as GPU temporal support.

### Avoid applying the display model twice

The official PQ route clips decoded PQ luminance and adds intrinsic black and
reflected ambient. Its linear route clips absolute input and adds reflected
ambient. HDR-VDP expects photometrically prepared input instead. Therefore a
single anonymous “linear HDR buffer” is not a sufficient interface between these
metrics. The preparation receipt must state whether clipping, exposure, black
and reflection have already been applied, and which stage owns each operation.
Passing a fully rendered buffer through CVVDP's display model again changes it.

## HDR-VDP-2

Official 2.2.2 consumes a same-size image pair, an explicit color encoding, ppd,
and options. Absolute luminance, linear BT.709 and XYZ routes require physical
units. Its `sRGB-display`/`luma-display` routes instead accept normalized encoded
values and impose a specific 100-nit/1-nit-black SDR model. They cannot stand in
for arbitrary user-selected displays. Spectral emission, surround and algorithm
options are part of the reproducible configuration.

The Rust [direct API](https://github.com/imazen/zenmetrics/blob/d7cced0f/crates/hdrvdp/src/metric.rs)
takes f64 slices; the v2 internal pipeline uses f32. Its
[encoding enum](https://github.com/imazen/zenmetrics/blob/d7cced0f/crates/hdrvdp/src/display.rs)
supports luminance, luma-display, sRGB-display, BT.709 linear and XYZ. It selects
D65 for luminance and CCFL-LCD for RGB/XYZ; it does not expose official custom
emission selection through this scoring API. The generic adapter accepts the
matching f32 gray/RGB descriptors and refuses XYZ. HDR BT.2020 must be converted
through a recorded supported route; it cannot be relabeled BT.709.

This is a stateless image metric. An EVIF sequence can feed individual frame
pairs, but the sampling and clip aggregation belong to a named wrapper. Retain
the selected output: v2 `q`, `p_det`, `c_max` and maps have different meanings.

## HDR-VDP-3

The official `hdrvdp3(task, test, reference, color_encoding, ppd, options)` needs
the same physical calibration plus task, observer age, surround and emission
configuration. It exposes `quality`, `side-by-side`, `flicker`, `detection` and
`civdm` comparisons, and selectable eye-optics MTF (`hdrvdp`, `cie`, `none`).
The [official task descriptions](https://hdrvdp.sourceforge.net/wiki/index.php/Main_Page)
distinguish these purposes. The `flicker` task compares two images alternated
every half-second; it does not accept an arbitrary video timeline.

The current Rust [v3 parameters](https://github.com/imazen/zenmetrics/blob/d7cced0f/crates/hdrvdp/src/v3/params.rs)
require:

- `Task`: quality, side-by-side or flicker. Detection and CIVDM are not exposed.
- `ViewingConditions`: f64 ppd (at least 4), observer age, and a surround of
  none/mirrored, reference geometric mean, uniform value, or per-channel values.
- `InputEncoding`: luminance, luma-display, sRGB-display, absolute BT.709 or
  BT.2020 RGB, display-native RGB, XYZ, or generic channels.
- `Emission`: a named preset or custom wavelength samples and one emission
  column per image channel. Custom wavelengths must cover the port's 360–780 nm
  resampling range. RGB chromaticities/CICP do not identify this spectrum.
- Algorithm options, including age-related mechanisms, masking, thresholds,
  sensitivity correction and floating-point mode.

The direct [v3 call](https://github.com/imazen/zenmetrics/blob/d7cced0f/crates/hdrvdp/src/v3/metric.rs)
takes interleaved f64 samples. It has no `Metric` implementation for invoking it
through the shared trait, although its result implements `Evaluation`. Its
[optical pathway](https://github.com/imazen/zenmetrics/blob/d7cced0f/crates/hdrvdp/src/v3/pathway.rs)
uses the fixed HDR-VDP MTF; the public options do not select CIE or disable it.
EVIF can describe these upstream choices, but the Rust adapter must refuse those
it cannot execute. Preserve result diagnostics such as gamut clipping and
relative-looking input; refuse nonfinite inputs before an implicit repair can
be mistaken for an exact replay.

The [published video procedure](https://arxiv.org/html/2304.13625#S6) runs the
quality task on sampled frames and averages their scores. It does not model
temporal interactions. Store sampled frame IDs/times, phase/step, display
preparation and the aggregation formula. Do not confuse this wrapper with
CVVDP's temporal filtering and feature pooling. The 3.0.7 example's loop has
fixed `frame_0001` paths despite a changing loop index; the EVIF driver must bind
each call to its declared frame identities, not copy that example literally.

## Required additions to the proposal's implementation contract

1. **Per-input preparation binding.** Identify the source and candidate assets,
   input signal domain/units, integer normalization, alpha background, spatial
   resize and temporal alignment. Bind presentation/rendering per input; a common
   presentation is a shared reference, not a ban on separately rendered inputs.
2. **Resolved photometry receipt.** Record exposure, intrinsic black, reflected
   light, ambient lux/reflectivity, EOTF and HLG system treatment, clipping/gamut
   behavior and operation order. Distinguish input from already-rendered samples.
3. **Typed observer/optics/surround record.** Include age and enabled mechanisms,
   MTF identity/options, and the surround's mode, units, channel domain and values.
   These are not interchangeable with ambient illuminance.
4. **Spectral dependency.** A preset name plus implementation/table hash, or an
   embedded/content-addressed wavelength-and-channel table with normalization and
   interpolation rules. Preserve f64 configuration precision where the scorer
   takes f64; the shared f32 `ViewingConditions` summary is insufficient alone.
5. **Adapter capability and call receipt.** Pin implementation/model/configuration
   resources; record task, accepted encoding, effective scalar types and rate,
   temporal options, exact frame coverage, output key and pooling. Unsupported
   settings must fail before scoring rather than disappear into `extra` JSON.

These additions are small compared with raster storage and do not require
duplicate decoded images. F16/F32 or integer source samples can be deterministically
widened to f64 for HDR-VDP-3; f64 arithmetic does not require an f64 rendition.
Preserving an arbitrary f64 source, or arbitrary multispectral image channels,
would need additional carrier profiles and must remain explicitly unsupported
until those profiles exist.

With these records, both available input sequences, and native-depth adapters,
the proposed format can supply the ordinary RGB HDR cases. Sufficient metadata
is not yet an end-to-end implementation: the final gate is an actual EVIF decode
and replay through official CVVDP, the Rust CPU video port, and explicit HDR-VDP
frame wrappers, checking their delivered inputs and effective settings.
