# Trade ladders, gauntlet grid, training-leg partition, diagnostic roles and pilot, 2026-10-30: data pointer

The summary is [trade-gauntlet-2026-10-30.md](trade-gauntlet-2026-10-30.md). The gauntlet grid is the gate partition. The
training-leg partition and the diagnostic roles are descriptive runs stored bitstream-only (below); they enter no gate.

**Tower mirror (array disk, not the share's cache), verified 2026-10-30 and 2026-10-31:** each tree was copied by tar streams,
then its file count, byte total and a 40-file SHA-256 sample were compared with the source. Root
`output/evif-chroma-trade-archive-2026-10-30/` on the NAS array disk 1:

| tree | files | bytes |
|---|---|---|
| `gauntlet-cas/` (the gauntlet store below) | 29,572 | 306,727,382,641 |
| `gauntlet-assembled/` (per-job ledgers and receipts) | 27,551 | 3,690,768,247 |
| `gate-gauntlet/` (result files named below) | 6 | 302,623,508 |
| `pilot-r3800x/` (pilot shards 00-03 and 08-13) | 215,325 | 73,341,625,050 |
| `pilot-i270/` (pilot shards 04-07) | 84,451 | 30,368,832,825 |
| `trainleg-cas/` (every store object written after 2026-10-30 11:55Z: the training-leg and diagnostic runs) | 52,419 | 303,274,146,526 |
| `trainleg-assembled/` (per-job ledgers and receipts of the training-leg run) | 69,751 | 15,282,719,349 |
| `diag-assembled/` (the same for the diagnostic run) | 5,081 | 1,077,945,318 |
| `gate-diag-r3800x/` (diagnostic result files) | 6 | 90,708,997 |

`trainleg-cas/` was copied in two verified parts: 50,599 files (296,201,119,861 bytes), then 1,820 files (7,073,026,665 bytes)
that the diagnostic run moved into the store after the first listing. Coverage check: all 81,991 files of the store on r3800x
(29,572 + 52,419) are present in `gauntlet-cas/` or `trainleg-cas/` with equal size. The three ledger checksum lists named
below sit in the archive root, and every per-job ledger on the tower copies was re-hashed against them.

- **Gauntlet grid** (2,755 jobs over 1,691 views, 173 sources): content-addressed store on r3800x,
  `~/chromatrade-data/grid-cas/<aa>/<bb>/<sha256>` (29,572 files, 285 GiB at the time of the gauntlet; the same store later
  received the objects of the two runs below). Each job's `result.json` names its ledger, receipts and tar packs of `.evif`,
  bitstreams and maps with a SHA-256 per member. Per-job ledgers and receipts:
  `~/chromatrade-data/gauntlet-assembled/<view>__<template>/` (3.6 GB; 1,952,564 rows). Checksums of the ledgers:
  `~/chromatrade-data/LEDGERS.sha256` (2,755 lines), itself `3f3faa3341971e68fd09051e19c447df070781f7c342b02573b2abc45ad3a1a9`.
- **Training-leg partition** (6,975 views of the TRAIN roles whose source hash mod 5 is not zero; every sample is its encoded
  bytes plus a ledger row, no `.evif`): jobset `chroma-trade-trainleg-20261030`, manifest
  `a5762dba5e0b6ad041cec4a4404caf80144da8db01e5024f0b9bdb7b6da215d5`. 6,975 jobs, 7,151 done rows (176 duplicate executions),
  7,979,400 ledger rows. Objects in the store above; per-job ledgers `~/chromatrade-data/trainleg-assembled/<view>__trainleg-r3/`;
  `~/chromatrade-data/LEDGERS-trainleg.sha256` (6,975 lines), itself
  `4df54417f9f13e326dee4db218c18bb77e99dc1eb6958b436bcccc072ea53fcb`. Store check: 6,530,272 pack members hash-verified
  (292.2 GB), 6,371,698 bitstream members (271.3 GB), every non-refused ledger row has its bitstream, 0 problems.
- **Diagnostic roles** (dev and calibration, 62 TRAIN sources; 60 family jobs over 48 gauntlet-partition views with `.evif`
  output and 448 training-leg views bitstream-only): jobset `chroma-trade-diag-20261030`, manifest
  `0fd8775196f174eec0122e11906fc568e46941a313fc3b5bc458af594d59fdf2`. 508 jobs, 567,676 ledger rows, per-job ledgers
  `~/chromatrade-data/diag-assembled/`; `~/chromatrade-data/LEDGERS-diag.sha256` (508 lines), itself
  `bd1fd66d6234f537f2b9e200cbf7e6d768f2988a7c081ffae712c6065e841150`. Store check: 525 done rows, 478,868 pack members
  hash-verified (20.6 GB), 0 problems.
- **Pilot** (81 sources, 162 views, 14 shards, 185,520 rows, loose `.evif` files): r3800x and i270, `~/chromatrade-data/pilot`
  (155 GB).
- **Result files** (gauntlet): `TALLIES.json` f10a5566e33f9486c805590a08f253f701d1b5d92f6b7e018bf2726cc62c7835,
  `BOOTSTRAP.json` d0eadb7ab404d0713d197eda2e128d0d0fc07e3db766290103a449dd14d3c6b6, verification receipt
  d553ea62db42cdeef143e14da17edfb5602319f9790d4abf630e6d1009834126, EVIF sample receipt
  ddec376c21df0221fa36f4f6df367e9651d521dc1a7d6e2121cd514917eb90cf.
- **Pins** (all three grid-style runs): grid binary SHA-256 `b7d6d89c09b8598fa3161b401b46ea50285cb0bec49970c47f85cbd0922584d4`
  (evif `bbf16802`), gauntlet jobset manifest `6462cb71f817b5676dd76f6af59af53fcd3b3b6a9c3d4cf7dc04cdbc3b004829`, view size
  table `56165e45e0d0f440bf7f2502e1812a013bfef4cd5d86f81c1ade4ffa7143dbb6`.
- **Reproduce the figures**: assemble the per-job ledgers from the store (`tools/trade-checks/assemble_grid.py`), then
  `python3 tools/trade_gate.py <job dirs> --dims <view size table> --out OUT` and `evif trade-bootstrap OUT/TALLIES.json
  BOOT.json`. The analysis needs about 6 GB of memory and three minutes for the gauntlet.
