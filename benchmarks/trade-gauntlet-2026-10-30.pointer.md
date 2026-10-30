# Trade ladders, gauntlet grid and pilot, 2026-10-30: data pointer

MISSING: the verified tower mirror (copy in progress; this line is replaced when the mirror's count, byte total and 40-file
SHA-256 sample have been checked) and the training-leg partition (running). The summary is
[trade-gauntlet-2026-10-30.md](trade-gauntlet-2026-10-30.md).

- **Gauntlet grid** (2,755 jobs over 1,691 views, 173 sources): content-addressed store on r3800x,
  `~/chromatrade-data/grid-cas/<aa>/<bb>/<sha256>` (29,572 files, 285 GiB). Each job's `result.json` names its ledger,
  receipts and tar packs of `.evif`, bitstreams and maps with a SHA-256 per member. Per-job ledgers and receipts:
  `~/chromatrade-data/gauntlet-assembled/<view>__<template>/` (3.6 GB; 1,952,564 rows). Checksums of the ledgers:
  `~/chromatrade-data/LEDGERS.sha256` (2,755 lines), itself `3f3faa3341971e68fd09051e19c447df070781f7c342b02573b2abc45ad3a1a9`.
- **Pilot** (81 sources, 162 views, 14 shards, 185,520 rows, loose `.evif` files): r3800x and i270, `~/chromatrade-data/pilot`
  (155 GB).
- **Result files**: `TALLIES.json` f10a5566e33f9486c805590a08f253f701d1b5d92f6b7e018bf2726cc62c7835, `BOOTSTRAP.json`
  d0eadb7ab404d0713d197eda2e128d0d0fc07e3db766290103a449dd14d3c6b6, verification receipt
  d553ea62db42cdeef143e14da17edfb5602319f9790d4abf630e6d1009834126, EVIF sample receipt
  ddec376c21df0221fa36f4f6df367e9651d521dc1a7d6e2121cd514917eb90cf.
- **Pins**: grid binary SHA-256 `b7d6d89c09b8598fa3161b401b46ea50285cb0bec49970c47f85cbd0922584d4` (evif `bbf16802`), jobset
  manifest `6462cb71f817b5676dd76f6af59af53fcd3b3b6a9c3d4cf7dc04cdbc3b004829`, view size table
  `56165e45e0d0f440bf7f2502e1812a013bfef4cd5d86f81c1ade4ffa7143dbb6`.
- **Reproduce the figures**: assemble the per-job ledgers from the store (`tools/trade-checks/assemble_grid.py`), then
  `python3 tools/trade_gate.py <job dirs> --dims <view size table> --out OUT` and `evif trade-bootstrap OUT/TALLIES.json
  BOOT.json`. The analysis needs about 6 GB of memory and three minutes.
