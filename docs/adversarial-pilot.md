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
both arms and both target profiles. The existing SVT adapter clamps the nominal
q0 point to quality 1; the other 20 points keep their requested qualities.
Search, seed, duplicate and baseline
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

`tools/adversarial_summary.py` writes separate `TRAINING-LEG.jsonl` and
`GAUNTLET.jsonl` find indexes, deduplicated by source/view/encoded SHA256, and
`PARTITIONS.json` reserves every source. Counts still use each arm's full
encode budget. Pass `--output NEW_DIRECTORY` to repeat a summary without
replacing an earlier receipt.

Readback accepts `--jobs 1..4` and an optional fresh `--output` directory.
Each decoder stays single-threaded; one run-heavy scope and CPU affinity
bound the whole audit. It independently decodes every retained bitstream,
including duplicates, and checks exact container coverage, score doubles,
reference/candidate map bindings and RGB8 dimensions. Native verification's
complete output is retained in `NATIVE_VERIFY.log`.
It also extracts the unchanged candidate bytes to
`artifacts/<codec>/<sha256>.<extension>`, authenticates existing CAS entries,
and independently decodes those persisted files. The scored EVIF remains the
authoritative reference/candidate evaluation.

After readback and summary, `tools/adversarial_archive.py` authenticates every
EVIF, native codec artifact and map against those receipts, checks source
reservations, and retains the exact reference PNGs, served models, executable
and archive/readback tools. It writes a per-file SHA256 manifest and an
uncompressed tar outside the run tree, then verifies three reproducibly sampled
files from the tar. Existing seals and archives are never replaced. Supply the
observed binary SHA256; `--binary-commit` and `--tool-commit` record the exact
landed source cuts. Archive receipts include the archive size/SHA256 and manifest
SHA256 for subsequent tower/R2 mirror checks.

`tools/adversarial_mirror.py` checks those same three archive members through
exact byte ranges. A local mirror also receives a full-archive SHA256 check;
a signed HTTPS mirror must return HTTP206, the exact range/total size, matching
SHA256 metadata and matching sampled bytes. Remote verification does not claim
a full remote-payload SHA256 calculation. Signed URLs stay in private files and
are omitted from the resulting receipt. Every check writes a fresh receipt.

When staging a worker build, use `just adversarial-stage HOST DIRECTORY`.
Preserving an older source timestamp can hide an edit from Cargo if the copy
lands during compilation. The pilot smoke refused such a stale binary before
encoding; a forced fresh build passed the manifest, golden and teacher gates.

Native codec readback uses seekable CAS file input. The pinned FFmpeg demuxer
refuses a 71,636-byte pilot AVIF through stdin after probing (partial file at
offset 0x106), while the same SHA256 bytes decode successfully from their CAS
path to the expected 512×512 RGB24 raster. PNG renditions still use stdin.
The failed first full audit is retained; repeated audits use a fresh output
directory and keep every existing check. This changes input transport, not
encoded bytes or the independent decoder.
