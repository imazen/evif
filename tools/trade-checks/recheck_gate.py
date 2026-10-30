# Independent re-implementation of the registered (a) metric, written from the record text only (no import of
# trade_gate.py). Includes the 2026-10-30 amendment: on views with a long edge of 64 px or less (cap in the view id)
# SSIMULACRA2 does not vote, so butteraugli and CVVDP must both vote worse.
# usage: recheck_gate.py DIR   (DIR/shard-*/ledger.jsonl)
import json, glob, collections, re, sys
MARG = {"ssim2-frozen": 1.0, "butteraugli": 0.5, "cvvdp": 0.25}
MODELS = ("zensim-e", "zensim-e-codec", "zensim-f", "zensim-f-codec")


def vote(a, b, ref):
    d = b[ref] - a[ref]
    if ref == "butteraugli":
        d = -d           # lower is better
    if d < -MARG[ref]:
        return -1        # worse
    if d > MARG[ref]:
        return 1         # better
    return 0


def small(view):
    m = re.search(r"_m(\d+)$", view)
    return bool(m) and int(m.group(1)) <= 64


rows = collections.defaultdict(list)
for f in glob.glob(sys.argv[1] + "/shard-*/ledger.jsonl"):
    for line in open(f):
        r = json.loads(line)
        if r.get("kind") == "ladder" and "refused" not in r and r["ramp"] != "none":
            rows[(r["view"], r["trade_family"], r["ramp"])].append(r)
tot = collections.defaultdict(collections.Counter)
for (view, fam, ramp), rs in rows.items():
    rs.sort(key=lambda r: r["rung"])
    refs = ("butteraugli", "cvvdp") if small(view) else tuple(MARG)
    stratum = "small" if small(view) else "regular"
    for a, b in zip(rs, rs[1:]):
        sa, sb = a["scores"], b["scores"]
        luma = (sb["luma601-psnr"] - sa["luma601-psnr"] >= 0.02) or (sb["lumrel-psnr"] - sa["lumrel-psnr"] >= 0.02)
        chroma = sb["chroma601-psnr"] - sa["chroma601-psnr"] <= -0.02
        if not (luma and chroma):
            continue
        v = [vote(sa, sb, r) for r in refs]
        for key in ((fam, stratum), (fam, "pooled")):
            tot[key]["trade"] += 1
            if sum(1 for x in v if x == -1) >= 2:
                tot[key]["worse"] += 1
                for m in MODELS:
                    if sb[m] >= sa[m]:
                        tot[key]["inv:" + m] += 1
for stratum in ("regular", "small", "pooled"):
    print("==", stratum)
    for fam in ("ycc444", "ycc420", "xyb-extended", "xyb-capped", "svt-420"):
        c = tot[(fam, stratum)]
        print(fam, "trade pairs", c["trade"], "decided WORSE", c["worse"], {m: c["inv:" + m] for m in MODELS})
