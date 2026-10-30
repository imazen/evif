# Trade ladders: luma gain against chroma collapse

Registered 2026-10-30, after the owner saw the chroma-ladder results and **before any trade sample was
scored**. This page fixes what is generated, what counts as ground truth, and the G-TRADE gate. A change to
any definition after the first trade sample is scored needs a new dated section, not an edit of this one.

## Why

Chroma-only ladders hold luma quantization exactly fixed (luma gain zero, not tiny) and stop at
SSIMULACRA2's first tie or reversal. That removes the cases where luma is no proxy for quality: luma a
little better, chroma far worse, past the point where SSIMULACRA2 itself rates a greyed-out image above a
posterised one. A metric that leans on luma can reward such encodes. Trade ladders put them into EVIF as
a first-class stress capability, next to the random and differential-evolution searches.

## Trade families

Every trade family expands to explicit recipes of an existing EVIF family, so a sample replays through
`evif render --family union --params`. The chroma rung lists are the ones of the chroma-only ladders EVIF
already renders, so a rung here and its chroma-only twin differ only in luma.

| id | EVIF family | luma control | chroma control | chroma rungs |
|---|---|---|---|---|
| `ycc444` | `zenjpeg-expert`, q95, 4:4:4 | Y `quant_scale`, all four bands | Cb = Cr `quant_scale`, all bands | 16: 0, 0.5, 2^-0.5, 1, 2^0.5, 2, ... 64 |
| `ycc420` | `zenjpeg-expert`, q95, 4:2:0 | as above | as above | same 16 |
| `xyb-extended` | `zenjpeg-expert`, q95, raw XYB tables (order X/Y/B) | Y (component 1) `quant_scale` | X = B `quant_scale` | 30: 0.1 ... 5, then 6.4 ... 1024 |
| `xyb-capped` | `zenjpeg-color`, XYB, B quarter | `quality` above 95 (the builder has no luma distance scale: Y keeps the base distance) | X = B `chroma_scales` | 14: 0.1 ... 5 |
| `svt-420` | `zenav1-svt`, 4:2:0 | `quality` above 95 (lower base/luma qindex) | U = V `chroma_q` delta | 17: -64, -56, ... 56, 63 |

Raw-table multipliers up to 1024 follow the existing raw-XYB precedent. No family goes beyond the
largest value an existing ladder already uses (YCbCr 64, XYB 1024, SVT delta 63). Wider YCbCr values are
measured in a labelled range probe (see Pilot) and are not adopted by the grid.

### Luma ramps

A ramp sets the luma end point of a ladder of K rungs; rung k of the ramp uses the fraction k/(K-1), so
rung 0 of every ramp has no luma gain. The chroma lists start finer than the base encode and pass it
where they contain 1.

| ramp | expert luma multiplier at the last rung | `xyb-capped` quality at the last rung | `svt-420` quality at the last rung |
|---|---|---|---|
| `none` | 1 (the chroma-only control) | 95 | 95 |
| `tiny` | 0.8 | 96.2 | 96 |
| `medium` | 0.5 | 98.1 | 97.5 |
| `large` | 0.25 | 99.6 | 99 |

Expert multipliers interpolate geometrically; qualities interpolate linearly. Integer quantization
tables mean adjacent rungs can share a luma table. The size actually achieved is measured per rung and
reported; the ramp values are inputs, not claims.

### Luma and chroma diagnostics

Recorded for every sample, never used as a reference vote:

- `luma601-psnr`: PSNR (peak 255, 100 dB when identical) of Y' = 0.299 R' + 0.587 G' + 0.114 B' on
  the 8-bit sRGB code values of reference and decoded raster. For a BT.601 YCbCr codec this is the luma
  plane the codec quantized, whatever its chroma did.
- `luma601-ssim2`: frozen SSIMULACRA2 on the two Y' planes, each rounded to 8 bits and replicated to
  R = G = B.
- `lumrel-psnr`: PSNR of CIE relative luminance (BT.709 weights in linear light), re-encoded with the sRGB
  transfer function, on 8-bit code values.
- `chroma601-psnr`: PSNR over the full-range BT.601 Cb and Cr planes (pooled MSE of both planes).

A **trade pair** is two adjacent rungs of a ramp other than `none`, where `luma601-psnr` strictly
increases and `chroma601-psnr` strictly decreases from the first to the second. A ladder is
**admitted** when it has at least three trade pairs. No ladder stops at an SSIMULACRA2 reversal.
Wash-out is reported against the chroma-only reference point: the first rung where the frozen teacher
stops falling by at least one point from the last such rung.

## Byte-matched pairs

For each view, codec family and byte budget, one encode per allocation ratio is solved to the same size
(within 1 %, at most eight encodes per cell, secant search on log size against `quality`). Budgets are
the sizes of the honest curve at qualities 25, 55 and 85. An allocation ratio rho multiplies the
luma and chroma quantization by rho^-1/2 and rho^1/2 (expert families), so rho above 1 is luma-heavy and
below 1 chroma-heavy: rho in {1/4, 1, 4, 16, 64}. `xyb-capped` uses the chroma scale itself, {0.25, 1, 4, 5};
`svt-420` uses chroma deltas {-32, 0, 32, 63}. A cell that cannot reach its budget is recorded as unmatched
and excluded from pairs. Every pair of matched cells at one budget is a byte-matched pair.

## Differential-evolution search

Same settings and budget rules as the chroma pilot: DE/rand/1/bin, six members, differential weight 0.8,
crossover 0.9, 16 evaluations per arm, view, family and target, random search on the same bounds, seed
stream and initial draws. Vectors are normalized cubes.

| family | dimensions |
|---|---|
| `ycc444`, `ycc420` | luma multiplier 2^-2x in [1/4, 1]; eight chroma band multipliers in [1/64, 64] (Cb bands, then Cr) |
| `xyb-extended` | luma multiplier in [1/4, 1]; X and B multipliers, log-uniform in [0.1, 1024] |
| `xyb-capped` | X and B scales in [0.1, 5]; quality in [95, 99.6] |
| `svt-420` | U and V deltas in [-64, 63] (rounded); quality in [95, 99] |

Targets are the profiles under test. The honest curve is the family's default encoder over 21 qualities
(0 to 100, step 5), shared by the trade families of one codec family. No admitted ladder seeds either arm.

## Ground truth (registered)

SSIMULACRA2 is unreliable in the wash-out region, so it cannot decide alone there. Three independent
references vote; no other score votes, and a second implementation of one metric is not a second vote.

| reference | score | worse means | vote margin |
|---|---|---|---|
| S | frozen SSIMULACRA2 0.8.2 (`ssim2-frozen`) | lower | 1.0 point |
| B | butteraugli max norm (`butteraugli`) | higher | 0.5 |
| C | CVVDP, CPU port, `standard_4k` (`cvvdp`) | lower | 0.25 JOD |

For an ordered pair (a, b) of samples of one view, a reference votes **worse** when b scores worse than
a by more than its margin, **better** when b scores better by more than its margin, and abstains
otherwise. The pair's truth is WORSE when at least two references vote worse, BETTER when at least two
vote better, and UNDECIDED otherwise; because two distinct references must agree, the agreeing set is
never SSIMULACRA2 alone. Every pair stores the three votes and the dissenting references. The margins
are fixed here; ledgers also record results at margins x0, x0.5 and x2 as sensitivity, which are not
gates.

For a model, a score that is not lower on the worse side is an **inversion**: truth WORSE and
`score(b) >= score(a)`. (A reverse inversion, truth BETTER and `score(b) <= score(a)`, is reported
separately.) Models are the revision-7 profiles E and ECodec through the pinned research adapter, plus
any candidate bake that adapter accepts; a bake it refuses is reported as refused.

The **find** rule for search samples extends the chroma pilot's. With gain the log2 of equivalent over
actual bytes on the honest curve, a sample is a find for target t when at least two of the three
margins `gain(t) - gain(S)`, `gain(t) - gain(B)`, `gain(t) - gain(C)` are positive. The search objective
is the second-largest of the three. The chroma pilot's two-reference margin is recorded beside it.

## G-TRADE (registered 2026-10-30 after the owner saw chroma results)

Evaluated on the **gauntlet partition only**: sources whose identity hash modulo 5 is zero (SHA-256 of
`{width}x{height}:{SHA-256 of the original decoded pixels}`, first eight bytes big-endian). Those
sources stay reserved against fitting. No model is fit to trade data.

- (a) **Trade-ladder inversions.** Over trade pairs with truth WORSE: the share of inversions, per source,
  then pooled and averaged over sources.
- (a2) **Byte-matched inversions.** The same share over byte-matched pairs with a decided truth.
- (b) **DE find rate.** Finds per 1,000 DE encodes for the model as target, per source; the random arm is
  the non-adaptive baseline.

Each is reported for E and ECodec absolutely and, where scored, for candidate bakes. Comparison to E
uses a paired bootstrap over sources (10,000 draws, seed 20261030, percentile interval).

**Proposed bar (not adopted; the owner decides whether it blocks or only reports):** a candidate fails
if the 95 % interval of (candidate - E) lies entirely above zero on (a), (a2) or (b), and improves if it
lies entirely below zero. More inversions or finds is worse.

## Pilot, then grid

The pilot runs every trade family on the 162 frozen views (81 TRAIN sources) of the chroma pilot. It
reports, per family: luma-gain sizes achieved, how far chroma goes past wash-out, admitted ladders,
reference agreement, and E and ECodec inversion counts. A labelled range probe encodes `ycc444` and
`ycc420` out to multiplier 1024 on a fixed 12-view subset to measure where YCbCr chroma wash-out ends;
the grid does not adopt values above 64. The grid then covers the chroma-ladder views (TRAIN roles),
gauntlet-partition views first.

## Data rules

Sources are TRAIN only: never the human-rated references of any dataset and never a validation or test
bucket. Roles other than fit and new-train are diagnostic. Before a search, the frozen teacher
replays its stored controls bit for bit and each profile its stored golden scores; the CVVDP and
butteraugli judges replay stored diagnostic doubles for a fixed set of cells, and the `none` ramp
reproduces the encoded bytes of fixed chroma-only cells. A failed control refuses the run. Every sample
is an `.evif` with the decoded pixels, recipe, original bytes and all scores; butteraugli and CVVDP
maps are kept for ladder end points, byte-matched cells, finds and a 5 % stable hash sample.
