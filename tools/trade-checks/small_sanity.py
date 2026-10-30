#!/usr/bin/env python3
"""Reference sanity by view size: median scores of the unmodified-chroma rung 0 of the `none` ramp per size cap.

usage: small_sanity.py LEDGER_DIR_OR_JSONL... > table.md
Shows what each reference reports for the same near-lossless encode as the frame shrinks: SSIMULACRA2 collapses
at 32 and 64 px (coordinator finding); the amendment relies on butteraugli and CVVDP, so they are checked too.
"""
import json, re, statistics, sys
from collections import defaultdict
from pathlib import Path

rows = defaultdict(list)
paths = []
for a in sys.argv[1:]:
    p = Path(a)
    paths += [p] if p.is_file() else sorted(p.rglob("ledger.jsonl"))
for path in paths:
    for line in path.open():
        r = json.loads(line)
        if r.get("kind") != "ladder" or "refused" in r or r.get("ramp") != "none" or r.get("rung") != 0:
            continue
        m = re.search(r"_m(\d+)$", r["view"])
        cap = int(m.group(1)) if m else (256 if "crop" in r["view"] else None)
        rows[(cap, r["trade_family"])].append(r["scores"])

print("| family | cap (long edge) | views | SSIMULACRA2 | butteraugli max | butteraugli 3-norm | CVVDP JOD | E | ECodec |")
print("|---|---|---|---|---|---|---|---|---|")
for (cap, fam), sc in sorted(rows.items(), key=lambda t: (t[0][1], t[0][0] or 0)):
    med = lambda k: statistics.median(s[k] for s in sc if k in s)
    print(f"| {fam} | {cap} | {len(sc)} | {med('ssim2-frozen'):.1f} | {med('butteraugli'):.2f} | {med('butteraugli/3-norm'):.2f} | "
          f"{med('cvvdp'):.2f} | {med('zensim-e'):.1f} | {med('zensim-e-codec'):.1f} |")
