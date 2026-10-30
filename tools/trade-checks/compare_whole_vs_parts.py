#!/usr/bin/env python3
"""Check that a view run as one whole-view job and the same view run as family-subset part jobs
produce the same ledger rows (bit for bit, as JSON text).

usage: compare_whole_vs_parts.py WHOLE_LEDGER.jsonl PART_LEDGER.jsonl [PART_LEDGER.jsonl ...]
Baseline rows are repeated in every part (one codec family each); they must agree wherever they repeat.
Exit 1 on any difference.
"""
import json, sys
from collections import Counter

whole = [json.loads(l) for l in open(sys.argv[1])]
parts = [json.loads(l) for p in sys.argv[2:] for l in open(p)]
# Only the families (and codec baselines) the given parts cover are compared.
covered_f = {r["trade_family"] for r in parts if r.get("trade_family")}
covered_c = {r["codec_family"] for r in parts if r.get("codec_family")}


def covered(r):
    if r.get("trade_family"):
        return r["trade_family"] in covered_f
    return r.get("codec_family") in covered_c


whole = [r for r in whole if covered(r)]


def key(r):
    return json.dumps({k: r.get(k) for k in ("view", "kind", "trade_family", "codec_family", "ramp", "rung", "rho",
                                             "budget", "budget_quality", "target", "arm", "index", "quality")},
                      sort_keys=True)


def canon(r):
    return json.dumps(r, sort_keys=True)


def table(rows):
    t = {}
    for r in rows:
        t.setdefault(key(r), set()).add(canon(r))
    return t


tw, tp = table(whole), table(parts)
problems = []
for k in sorted(set(tw) | set(tp)):
    a, b = tw.get(k), tp.get(k)
    if a is None:
        problems.append(("only in parts", k))
    elif b is None:
        problems.append(("only in whole", k))
    elif len(b) != 1 or len(a) != 1:
        problems.append(("ambiguous key", k, len(a), len(b)))
    elif a != b:
        ra, rb = json.loads(next(iter(a))), json.loads(next(iter(b)))
        diff = sorted(f for f in set(ra) | set(rb) if ra.get(f) != rb.get(f))
        problems.append(("differs", k, diff))
by_kind = Counter(json.loads(k)["kind"] for k in tw)
print(json.dumps(dict(whole_rows=len(whole), part_rows=len(parts), keys_whole=len(tw), keys_parts=len(tp),
                      by_kind=dict(by_kind), problems=len(problems))))
for p in problems[:15]:
    print("PROBLEM", p)
sys.exit(1 if problems else 0)
