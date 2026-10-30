# Trade ladders on the gauntlet partition, 2026-10-30

`evif trade` over the chroma-ladder views (TRAIN roles), gauntlet partition only (source hash mod 5 == 0): 173 sources,
1,691 views, 2,755 jobs, 1,952,564 ledger rows. Registered rules: `docs/trade-ladders.md` (registration `8950be8d`,
amendments `c1b3f96b` and `5b37c2c4`). Grid binary built from `bbf16802`; gate tool `896a295d`. Models E and ECodec are the
served revision-7 profiles. Comparison between models is a paired bootstrap over sources (10,000 draws, seed 20261030,
percentile 95 % interval, pooled ratio of sums). Data: `trade-gauntlet-2026-10-30.pointer.md`.

Truth: a pair of one view is WORSE when two of {SSIMULACRA2, butteraugli, CVVDP} vote worse, never SSIMULACRA2 alone.
On views with a long edge of 64 px or less SSIMULACRA2 does not vote (amendment `5b37c2c4`, pushed before any grid figure
was read): butteraugli and CVVDP must both agree. An inversion is truth WORSE with `score(b) >= score(a)`.

## Adjacent trade pairs (a)

Views above 64 px (1,345 views; 18,267 pairs with truth WORSE):

| family | pairs | decided WORSE | E inverts | ECodec inverts |
|---|---|---|---|---|
| ycc444 (zenjpeg-expert, 4:4:4, luma finer, chroma x1 to x64) | 35,440 | 6,508 | 317 (4.9 %) | 322 (4.9 %) |
| ycc420 | 38,201 | 6,115 | 202 (3.3 %) | 201 (3.3 %) |
| xyb-extended (raw XYB tables, X/B to x1024) | 39,975 | 5,607 | 869 (15.5 %) | 1,123 (20.0 %) |
| xyb-capped (zenjpeg-color, scales to 5) | 38,497 | 36 | 1 | 1 |
| svt-420 (zenav1-svt, U/V delta to +63) | 4,707 | 1 | 1 | 1 |
| all | 156,820 | 18,267 | 1,390 (7.61 %, [6.38, 8.95]) | 1,648 (9.02 %, [7.53, 10.63]) |

ECodec minus E: +1.41 points [+0.43, +2.48].

Views of 64 px or less (346 views; 858 pairs with truth WORSE under the butteraugli + CVVDP rule): E inverts 75 (8.74 %, [5.65,
12.13]), ECodec 109 (12.70 %, [9.02, 16.73]); difference +3.96 points [+0.34, +7.73].

Ladders (ramps other than `none`) on which the model inverts at least one decided-WORSE pair, of those holding one: ycc444 E
11.3 % (310 of 2,740), ECodec 10.9 %; ycc420 7.5 % (194 of 2,580), 7.6 %; xyb-extended 26.5 % (824 of 3,105), 31.8 %.

## Byte-matched pairs (a2) and DE finds (b)

| stratum | metric | E | ECodec | ECodec - E |
|---|---|---|---|---|
| above 64 px | (a2) inversion share, 51,361 decided-WORSE pairs | 1.19 % [0.97, 1.42] | 1.38 % [1.11, 1.70] | +0.19 points [-0.01, +0.43] |
| above 64 px | (b) finds per 1,000 DE encodes, 107,600 encodes | 461.6 [443.5, 479.4] | 446.0 [424.9, 465.9] | -15.7 [-27.9, -3.6] |
| above 64 px | (b) random arm | 406.7 [391.3, 421.9] | 393.1 [374.7, 410.4] | -13.7 [-23.8, -3.9] |
| 64 px or less | (a2) 4,625 pairs | 0.43 % | 0.52 % | +0.09 points, not distinguishable |
| 64 px or less | (b) 27,680 encodes, both margins positive | 206.8 [192.5, 221.7] | 218.8 [204.2, 233.8] | +12.0 [+1.2, +23.0] |

A find needs at least two of three margins above zero (both valid references on small views), the margin being the
equivalent-bitrate gain of the model minus that of the reference.

## Reference levels by view size (rung 0 of the `none` ramp, medians)

| family | long edge | SSIMULACRA2 | butteraugli max | CVVDP | E |
|---|---|---|---|---|---|
| ycc444 | 32 / 64 / 128 / 960 | 93.8 / 93.2 / 93.8 / 91.6 | 0.52 / 0.63 / 0.72 / 0.94 | 10.00 | 93.1 / 93.3 / 92.6 / 91.6 |
| xyb-extended | 32 / 64 / 128 / 960 | 78.4 / 73.9 / 92.4 / 90.9 | 0.51 / 0.63 / 0.73 / 0.95 | 9.99 | 91.0 / 91.7 / 91.3 / 90.7 |

## Checks

Every job passed its controls (teacher replay, CVVDP and butteraugli replay, 16 stored chroma-only encodes). The store was
verified end to end (2,898 done rows, 41,229 object references, 1,961,834 pack members hash-checked, 308.3 GB, no problem);
1,000 sampled EVIFs matched their ledger rows bit for bit and passed `evif verify`. A second implementation of (a) written from
the record text gives the same counts. The pilot (81 sources, 162 views at 256 and 512 px) is the descriptive companion; its
numbers agree in direction with the grid.
