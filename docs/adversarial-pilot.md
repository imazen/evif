# SDR chroma adversarial pilot

The opt-in `adversarial,possession-svt` probe compares differential evolution
with random sampling at the same encode budget. It scores both revision-7
models through pinned zensimx custom profiles, the exact SafeSyn SSIMULACRA2
teacher, and butteraugli. Existing probe commands and judges remain available.

The objective is the smaller of `gain(target) - gain(ssim2-frozen)` and
`gain(target) - gain(butteraugli)`. Both differences must be strictly positive
for a find. Butteraugli's 3-norm is recorded but does not get a second vote.
Other disagreements remain in the ledger. This measures metric agreement,
not human preference.

Each view/family shares a 21-point honest quality curve (0–100, step 5) across
both arms and both target profiles. Search, seed, duplicate and baseline
encodes are reported separately. Every encode retains original codec bytes,
exact decoded PNG pixels, raw scores and model identities in an EVIF. Native
butteraugli maps are pixel-bound PFM files; the frozen teacher has no native
map API, so no proxy map is substituted.

Quality stays at 95, AQ on, other controls at family defaults. Color varies
XYB X/B distance scales over 0.1–5; expert varies eight YCbCr chroma-band
multipliers over 1/64–64 with unit luma multipliers and 4:4:4; SVT varies U/V
chroma deltas over −64–63 with baseline luma and 4:2:0. This is a chroma-first
pilot, not exhaustive coverage of the families' mode combinations.

DE/rand/1/bin uses six population members, differential weight 0.8 and
crossover probability 0.9, with at least one crossed coordinate. Parameters
use a normalized cube, logarithmic JPEG scales and rounded SVT deltas.
Trials replace a parent only on an improved two-reference margin. DE handles
bounded, non-differentiable, interacting parameters without a new dependency.
Six members and 16 evaluations make this a small-budget pilot, not evidence
of convergence. Random uses identical bounds, seeds, initial draws and budget.
Both arms spend their first evaluations on at most two admitted ladder rungs.
Unadmitted JPEG ladders provide no seed; A2 has no admitted SDR SVT seeds.

Sources must already be TRAIN. The program authenticates each view and original
file and unifies exact original-pixel aliases by geometry plus SHA256. Hash
that identity again, interpret its first eight bytes as big-endian, and take
modulo five: zero selects the gauntlet, otherwise the training leg. Every size
and finding from that source inherits its partition. The training leg's target
is the frozen teacher. No training runs here. These originally TRAIN sources
may be searched to construct the gauntlet; reserve the resulting gauntlet
sources against future fitting.

```sh
just adversarial-check
just adversarial-build
export EVIF_E_MODEL=/data/rev7_general_h128_2026-10-27.bin
export EVIF_ECODEC_MODEL=/data/rev7_codec_h128_2026-10-27.bin
just adversarial-prepare /data/chroma-study /data/pilot.json COMMIT
just adversarial-run /data/pilot.json /data/new-run
```

The adapter requires exact bake SHA256s: general
`e460a214c1e0c4f48bc7e3bb4705df548f9f172807cc63f39ba17cb1d94e05a0`,
codec `f7ecd60299482210fe33586e8d4674fe9ba163f17f2dbaf54c6e99d5ef44af63`.
These are the metadata-stripped bakes in zensim draft
`aad4c311d850defb8d3224bb2f963779bfdd1123`. The adapter follows the custom
profile in zensimx `split_parity_golden` at
`a4a00e250b89c1217a110100f16c6b1e1ef2f929`.
The teacher is exact registry version 0.8.2, published source
`b7c2b4b33f3a26b753caabd5264a7fef9c4f4545`, not the current git judge.
Before searching, four stored controls must match score bits and both pixel
hashes. A failed control refuses the pilot.

The command requires a fresh directory, flushes each ledger row, writes
per-arm wall-time receipts and `PROGRESS.json`, and emits `COMPLETE.json` only
after every arm completes. Encode errors fail rather than drawing replacements
outside the budget. Preserve incomplete outputs; interrupted timings are invalid.
This local single-process command does not implement a fleet scheduler;
distributed execution belongs in zenfleet.
