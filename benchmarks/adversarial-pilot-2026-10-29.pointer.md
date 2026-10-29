# SDR adversarial pilot, 2026-10-29

MISSING: sealed archive and final private mirrors. The owner stopped work after
the current readback step; no training or expanded search was started. The
committed table is the completed generation/ledger result. Consult the run's
`ORACLE.json` for the final independent audit receipt; do not infer it from a
partial progress file.

162 frozen TRAIN views from 81 original sources, 16 search encodes per
view/family/target/method, 31,104 search encodes plus 10,206 shared baseline
encodes. Each aggregate row has 2,592 encodes per method. Counts deduplicate
view/encoded SHA256 within each method; seed and duplicate attempts still spend
budget. The [TSV](adversarial-pilot-2026-10-29.tsv) contains all five coarse
content classes and arm wall times.

| Target | Family | DE finds/1,000 | Random finds/1,000 | DE seconds | Random seconds |
|---|---|---:|---:|---:|---:|
| zensim-e | zenjpeg-color | 182.484568 | 152.006173 | 444.453788 | 442.285604 |
| zensim-e | zenjpeg-expert | 318.287037 | 244.984568 | 408.369678 | 407.202804 |
| zensim-e | zenav1-svt | 288.966049 | 248.842593 | 489.486433 | 488.019192 |
| zensim-e-codec | zenjpeg-color | 181.327160 | 177.469136 | 445.451598 | 445.141193 |
| zensim-e-codec | zenjpeg-expert | 247.299383 | 190.200617 | 405.091070 | 406.818869 |
| zensim-e-codec | zenav1-svt | 313.657407 | 250.771605 | 490.107752 | 490.222779 |

DE wins all six aggregates and 28/30 coarse class combinations. Random wins
ECodec/color for line art/scan and screen/plot. Across the six groups, DE has
3,971 arm-deduplicated finds/15,552 encodes, random 3,277/15,552. The cross-arm,
cross-target source/view/encoded-SHA indexes contain 4,967 training-leg and 935
gauntlet candidates. Source roles reserve 66/15 original sources respectively.
These originally TRAIN sources are reserved against future fitting; this does
not assert that the served models never saw them. No new training admission.

These are two-reference metric disagreements, not human labels. The 16-point
budget and one deterministic run do not establish convergence or statistical
confidence. Arm times include encode/decode/scoring/persistence and exclude the
shared baselines. Generator internal wall: 7020.632497084 seconds.

```
run-heavy: done rc=0 7032s | peak-RSS 0.52GiB | min-avail 11616MiB | peak-load 22.34
```

Run checked on r3800x (local XFS, not another host's /mnt/v):
`/mnt/v/output/evif/adversarial-pilot-20261029/pilot-r1/`.
Repeated seekable-input readback directory:
`/mnt/v/output/evif/adversarial-pilot-20261029/pilot-readback-r2/`.
Generation source: `faf878e455c0f6d7e83f245f27271ebd5b036571`;
Rust search source: `d65551cdb0c34ab264dd2acdae79697944082b6b`.
Executable SHA256:
`92966a2e503e72804c5500f159d6d2aa25347af903ef9000d0cb5bdd1e82603a`.
Ledger SHA256:
`0302112e7e009961e628799e8e377702243bd0e27a84e48f1b9403fc9ad3617b`.

Every generated candidate and baseline retains its scored EVIF, original bytes
and exact scored PNG. Native verification checked all 41,310 containers. The
first independent audit stopped on a non-seekable AVIF input; the unchanged CAS
file decodes correctly through seekable input. Fix `cdf8d68b` preserves every
check and the failed attempt. Both the exact regression and 255-container smoke
pass. Full readback writes its success receipt only after all files pass.

See [method, pins and controls](../docs/adversarial-pilot.md). Both served model
profiles match all 28 stored golden bits; the frozen teacher replays all four
stored controls exactly. Planned private backup prefix (not a claim of a full
pilot upload): `s3://evif-runs/adversarial/2026-10-29/pilot-r1/`.
Next step, requiring a new authorization after the owner's stop: use the complete
readback and summary receipts to seal the run, then verify the private mirrors.
