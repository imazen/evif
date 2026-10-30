#!/usr/bin/env python3
"""Ledger -> truth votes, inversions, finds and the per-source tallies of G-TRADE.

Definitions are the registered ones in docs/trade-ladders.md (and its amendment). Nothing here
fits or tunes anything. The paired bootstrap over sources is `evif trade-bootstrap`
(zenmetrics-stats); this tool writes its input (`TALLIES.json`) and the descriptive tables.
"""
import argparse
import itertools
import json
import statistics
from collections import Counter, defaultdict
from pathlib import Path

REFS = ("ssim2-frozen", "butteraugli", "cvvdp")
LOWER_IS_BETTER = {"butteraugli"}
MARGIN = {"ssim2-frozen": 1.0, "butteraugli": 0.5, "cvvdp": 0.25}
MODELS = ("zensim-e", "zensim-e-codec", "zensim-f", "zensim-f-codec")
DIAG_STEP = 0.02  # dB, amended trade-pair criterion
FAMILIES = ("ycc444", "ycc420", "xyb-extended", "xyb-capped", "svt-420")


def votes(sa, sb, scale=1.0):
    """Each reference's vote on sample b relative to sample a: worse, better or abstain."""
    out = {}
    for r in REFS:
        quality_change = sb[r] - sa[r]
        if r in LOWER_IS_BETTER:
            quality_change = -quality_change
        m = MARGIN[r] * scale
        out[r] = "worse" if quality_change < -m else "better" if quality_change > m else "abstain"
    return out


def truth(v):
    """WORSE/BETTER when at least two distinct references agree, else UNDECIDED."""
    worse = sum(x == "worse" for x in v.values())
    better = sum(x == "better" for x in v.values())
    if worse >= 2:
        result = "WORSE"
    elif better >= 2:
        result = "BETTER"
    else:
        return "UNDECIDED", []
    agreeing = [r for r, x in v.items() if x == result.lower()]
    # Two distinct references agree, so the agreeing set is never SSIMULACRA2 alone.
    assert len(agreeing) >= 2 and agreeing != ["ssim2-frozen"]
    return result, agreeing


def model_inversions(sa, sb, result, models):
    """Inversion: truth WORSE and the model does not score b lower. Reverse: BETTER and not higher."""
    flags = {}
    for m in models:
        if m not in sa or m not in sb:
            continue
        if result == "WORSE":
            flags[m] = "inversion" if sb[m] >= sa[m] else "ok"
        elif result == "BETTER":
            flags[m] = "reverse" if sb[m] <= sa[m] else "ok"
    return flags


def admission(values):
    """The chroma-only ladders' admission walk (Stage B rule): stop at the first adjacent tie or
    increase; keep a rung only when it falls at least one point from the last kept rung."""
    kept = [0]
    stop = None
    for i in range(1, len(values)):
        if values[i] >= values[i - 1]:
            stop = i
            break
        if values[i] <= values[kept[-1]] - 1.0:
            kept.append(i)
    return kept, stop


class Tally:
    """Per source counters for one gate metric and model."""

    def __init__(self):
        self.data = defaultdict(lambda: defaultdict(lambda: defaultdict(Counter)))

    def add(self, source, stratum, model, **counts):
        self.data[source][stratum][model].update(counts)


def read_rows(runs):
    for run in runs:
        with (Path(run) / "ledger.jsonl").open() as f:
            for line in f:
                yield json.loads(line)


def trade_pair(a, b):
    d = lambda k: b["scores"][k] - a["scores"][k]
    luma_up = d("luma601-psnr") >= DIAG_STEP or d("lumrel-psnr") >= DIAG_STEP
    chroma_down = d("chroma601-psnr") <= -DIAG_STEP
    return luma_up and chroma_down, dict(luma601=d("luma601-psnr"), lumrel=d("lumrel-psnr"),
                                         chroma601=d("chroma601-psnr"), dbytes=b["bytes"] - a["bytes"])


def analyse(rows, models):
    """Walk the ledger once; return tallies and the descriptive tables."""
    ladders = defaultdict(list)
    bytematch = defaultdict(list)
    de = defaultdict(lambda: dict(encodes=0, refused=0, finds=set(), legacy=set()))
    probe = defaultdict(list)
    refused = Counter()
    bytematch_cells = Counter()
    partitions = {}
    for r in rows:
        partitions[r["source"]] = r["partition"]
        if "refused" in r:
            refused[r.get("kind", "?")] += 1
            if r.get("kind") == "de":
                de[(r["view"], r["trade_family"], r["target"], r["arm"])]["refused"] += 1
            continue
        kind = r["kind"]
        if kind == "bytematch":
            bytematch_cells[(r["partition"], r["trade_family"], bool(r.get("matched")))] += 1
        if kind == "ladder":
            ladders[(r["view"], r["trade_family"], r["ramp"])].append(r)
        elif kind == "bytematch" and r.get("matched"):
            bytematch[(r["view"], r["trade_family"], r["budget_quality"])].append(r)
        elif kind == "de":
            cell = de[(r["view"], r["trade_family"], r["target"], r["arm"])]
            cell["encodes"] += 1
            cell["source"] = r["source"]
            cell["partition"] = r["partition"]
            if r["find"]:
                cell["finds"].add(r["encoded_sha256"])
            if r["legacy_find"]:
                cell["legacy"].add(r["encoded_sha256"])
        elif kind == "probe":
            probe[(r["view"], r["trade_family"])].append(r)

    tally = {"a": Tally(), "a2": Tally(), "b": Tally(), "a_s2": Tally(), "a_s4": Tally(), "a_s8": Tally()}
    agree = Counter()
    examples = []
    bm_stats = Counter()
    luma_gain = defaultdict(list)
    ladder_rows = []
    pair_rows = []
    for (view, tf, ramp), rs in sorted(ladders.items()):
        rs.sort(key=lambda r: r["rung"])
        source = rs[0]["source"]
        trade_pairs = 0
        scores_s = [r["scores"]["ssim2-frozen"] for r in rs]
        kept, stop = admission(scores_s)
        for a, b in zip(rs, rs[1:]):
            criterion, deltas = trade_pair(a, b)
            is_trade = ramp != "none" and criterion
            v = votes(a["scores"], b["scores"])
            t, agreeing = truth(v)
            flags = model_inversions(a["scores"], b["scores"], t, models)
            pair_rows.append(dict(view=view, source=source, partition=a["partition"], family=tf, ramp=ramp,
                                  rung_a=a["rung"], rung_b=b["rung"], trade=is_trade, truth=t, votes=v, **deltas))
            if not is_trade:
                continue
            trade_pairs += 1
            luma_gain[(tf, ramp)].append(max(deltas["luma601"], deltas["lumrel"]))
            part = a["partition"]
            for scope in (part, "all"):
                agree[(scope, tf, "trade-pairs")] += 1
                agree[(scope, tf, "decided")] += t != "UNDECIDED"
                agree[(scope, tf, "decided-worse")] += t == "WORSE"
                agree[(scope, tf, "unanimous")] += t != "UNDECIDED" and len(agreeing) == 3
                for r in REFS:
                    if t != "UNDECIDED" and r not in agreeing:
                        agree[(scope, tf, "dissent:" + r)] += 1
                if t == "WORSE" and v["ssim2-frozen"] == "better":
                    agree[(scope, tf, "ssim2-says-better-while-others-say-worse")] += 1
            if t == "WORSE" and any(f == "inversion" for f in flags.values()):
                examples.append(dict(kind="ladder", view=view, partition=part, family=tf, ramp=ramp, rung_a=a["rung"],
                                     rung_b=b["rung"], chroma_a=a["chroma"], chroma_b=b["chroma"], bytes_a=a["bytes"],
                                     bytes_b=b["bytes"], deltas=deltas, truth=t, agreeing=agreeing,
                                     refs_a={r: a["scores"][r] for r in REFS}, refs_b={r: b["scores"][r] for r in REFS},
                                     models_a={m: a["scores"][m] for m in models if m in a["scores"]},
                                     models_b={m: b["scores"][m] for m in models if m in b["scores"]},
                                     inverted=[m for m, f in flags.items() if f == "inversion"],
                                     excess={m: b["scores"][m] - a["scores"][m] for m in flags}))
            for m in models:
                if m in a["scores"]:
                    f = flags.get(m)
                    tally["a"].add(source, tf, m, pairs=1, undecided=int(t == "UNDECIDED"),
                                   n_worse=int(t == "WORSE"), inv=int(f == "inversion"),
                                   n_better=int(t == "BETTER"), reverse=int(f == "reverse"))
        # Exploratory strides (not the registered gate): the same trade-pair criterion and vote rule
        # between rungs two, four and eight apart, where changes clear the vote margins more often.
        for stride in (2, 4, 8):
            for a, b in zip(rs, rs[stride:]):
                criterion, deltas = trade_pair(a, b)
                if ramp == "none" or not criterion:
                    continue
                t, _ = truth(votes(a["scores"], b["scores"]))
                flags = model_inversions(a["scores"], b["scores"], t, models)
                for m in models:
                    if m in a["scores"]:
                        f = flags.get(m)
                        tally[f"a_s{stride}"].add(source, tf, m, pairs=1, undecided=int(t == "UNDECIDED"),
                                                  n_worse=int(t == "WORSE"), inv=int(f == "inversion"),
                                                  n_better=int(t == "BETTER"), reverse=int(f == "reverse"))
        first, last = rs[0], rs[-1]
        ladder_rows.append(dict(view=view, source=source, partition=rs[0]["partition"], family=tf, ramp=ramp,
                                rungs=len(rs), trade_pairs=trade_pairs, admitted=trade_pairs >= 3, kept=len(kept),
                                stop=stop, depth_past_stop=(len(rs) - 1 - stop) if stop is not None else 0,
                                chroma_at_stop=rs[stop]["chroma"] if stop is not None else None,
                                chroma_last=last["chroma"],
                                luma601_gain=last["scores"]["luma601-psnr"] - first["scores"]["luma601-psnr"],
                                lumrel_gain=last["scores"]["lumrel-psnr"] - first["scores"]["lumrel-psnr"],
                                chroma601_loss=first["scores"]["chroma601-psnr"] - last["scores"]["chroma601-psnr"]))
    for (view, tf, budget), rs in sorted(bytematch.items()):
        rs.sort(key=lambda r: r["rho"])
        source = rs[0]["source"]
        for a, b in itertools.combinations(rs, 2):
            v = votes(a["scores"], b["scores"])
            t, agreeing = truth(v)
            flags = model_inversions(a["scores"], b["scores"], t, models)
            bm_stats[(a["partition"], tf, "pairs")] += 1
            bm_stats[(a["partition"], tf, "decided")] += t != "UNDECIDED"
            if t == "WORSE" and any(f == "inversion" for f in flags.values()):
                examples.append(dict(kind="bytematch", view=view, partition=a["partition"], family=tf,
                                     budget_quality=budget, rho_a=a["rho"], rho_b=b["rho"], bytes_a=a["bytes"],
                                     bytes_b=b["bytes"], truth=t, agreeing=agreeing,
                                     refs_a={r: a["scores"][r] for r in REFS}, refs_b={r: b["scores"][r] for r in REFS},
                                     models_a={m: a["scores"][m] for m in models if m in a["scores"]},
                                     models_b={m: b["scores"][m] for m in models if m in b["scores"]},
                                     inverted=[m for m, f in flags.items() if f == "inversion"],
                                     excess={m: b["scores"][m] - a["scores"][m] for m in flags}))
            for m in models:
                if m in a["scores"]:
                    f = flags.get(m)
                    tally["a2"].add(source, tf, m, pairs=1, n_worse=int(t == "WORSE"), inv=int(f == "inversion"),
                                    n_better=int(t == "BETTER"), reverse=int(f == "reverse"),
                                    undecided=int(t == "UNDECIDED"))
    for (view, tf, target, arm), cell in de.items():
        if "source" not in cell:
            continue
        budget = cell["encodes"] + cell["refused"]
        tally["b"].add(cell["source"], f"{tf}/{arm}", target, encodes=budget, finds=len(cell["finds"]),
                       legacy_finds=len(cell["legacy"]))
    return dict(tally=tally, partitions=partitions, agree=agree, luma_gain=luma_gain, ladders=ladder_rows,
                pairs=pair_rows, probe=probe, refused=refused, examples=examples, de=de, bm_stats=bm_stats,
                bytematch_cells=bytematch_cells)


def tallies_json(result, models):
    """Per partition and gate metric: model -> source -> counts, pooled over strata."""
    out = {}
    for part in ("gauntlet", "training-leg", "all"):
        out[part] = {}
        for metric, tally in result["tally"].items():
            per = defaultdict(lambda: defaultdict(Counter))
            for source, strata in tally.data.items():
                if part != "all" and result["partitions"][source] != part:
                    continue
                for stratum, by_model in strata.items():
                    if metric == "b" and stratum.endswith("/random"):
                        key_metric = "b-random"
                    else:
                        key_metric = metric
                    for model, counts in by_model.items():
                        per[(key_metric, model)][source].update(counts)
            for (key_metric, model), by_source in per.items():
                out[part].setdefault(key_metric, {})[model] = {s: dict(c) for s, c in by_source.items()}
    return out


def table(rows, header):
    lines = ["| " + " | ".join(header) + " |", "|" + "---|" * len(header)]
    lines += ["| " + " | ".join(str(x) for x in r) + " |" for r in rows]
    return "\n".join(lines)


def percentile(values, p):
    if not values:
        return float("nan")
    s = sorted(values)
    k = (len(s) - 1) * p
    lo = int(k)
    hi = min(lo + 1, len(s) - 1)
    return s[lo] + (s[hi] - s[lo]) * (k - lo)


def sum_tally(tally, scope, partitions, model, stratum_filter=None):
    """Pool one gate metric's per-source counters over a scope (partition name or 'all')."""
    by_family = defaultdict(Counter)
    for source, strata in tally.data.items():
        if scope != "all" and partitions[source] != scope:
            continue
        for stratum, by_model in strata.items():
            if model in by_model:
                by_family[stratum].update(by_model[model])
    return by_family


def pct(x, n):
    return f"{100 * x / n:.1f}%" if n else "-"


def pilot_tables(result, models, scope="all"):
    """Descriptive pilot statistics (markdown) for one scope: a partition name or 'all'."""
    part = result["partitions"]
    out = [f"## Scope: {scope}"]
    lad = [r for r in result["ladders"] if scope == "all" or r["partition"] == scope]
    rows = []
    for tf in FAMILIES:
        for ramp in ("none", "tiny", "medium", "large"):
            ls = [r for r in lad if r["family"] == tf and r["ramp"] == ramp]
            if not ls:
                continue
            rows.append([tf, ramp, len(ls), sum(r["admitted"] for r in ls),
                         f"{statistics.median(r['luma601_gain'] for r in ls):.2f}",
                         f"{statistics.median(r['lumrel_gain'] for r in ls):.2f}",
                         f"{statistics.median(r['chroma601_loss'] for r in ls):.2f}",
                         f"{statistics.median(r['depth_past_stop'] for r in ls):.0f}",
                         sum(1 for r in ls if r["depth_past_stop"] > 0), sum(1 for r in ls if r["stop"] is not None)])
    out.append("### Ladders (admitted = at least three trade pairs)\n" + table(rows, [
        "family", "ramp", "ladders", "admitted", "median luma601 gain dB", "median lumrel gain dB",
        "median chroma601 loss dB", "median rungs past chroma-only stop", "ladders past stop", "ladders with a stop"]))
    rows = []
    for tf in FAMILIES:
        c = result["agree"]
        n = c[(scope, tf, "trade-pairs")]
        if not n:
            continue
        rows.append([tf, n, pct(c[(scope, tf, "decided")], n), pct(c[(scope, tf, "unanimous")], n),
                     c[(scope, tf, "decided-worse")]] + [c[(scope, tf, "dissent:" + r)] for r in REFS] +
                    [c[(scope, tf, "ssim2-says-better-while-others-say-worse")]])
    out.append("### Reference agreement on trade pairs (ramps other than none)\n" + table(rows, [
        "family", "trade pairs", "decided", "unanimous", "decided WORSE"] + [f"dissent {r}" for r in REFS] +
        ["S says better, B and C worse"]))
    for metric, label in (("a", "(a) trade-ladder inversions, adjacent rungs (registered)"),
                          ("a2", "(a2) byte-matched inversions"),
                          ("a_s2", "exploratory: trade pairs two rungs apart"),
                          ("a_s4", "exploratory: trade pairs four rungs apart"),
                          ("a_s8", "exploratory: trade pairs eight rungs apart")):
        rows = []
        for tf in FAMILIES:
            for m in models:
                c = sum_tally(result["tally"][metric], scope, part, m).get(tf)
                if c and c["pairs"]:
                    rows.append([tf, m, c["pairs"], c["undecided"], c["n_worse"], c["inv"], pct(c["inv"], c["n_worse"]),
                                 c["n_better"], c["reverse"], pct(c["reverse"], c["n_better"])])
        out.append(f"### {label}\n" + table(rows, ["family", "model", "pairs", "undecided", "decided WORSE",
                                                      "inversions", "rate", "decided BETTER", "reverse", "rate"]))
    rows = []
    for tf in FAMILIES:
        for target in models:
            for arm in ("de", "random"):
                enc = fin = leg = 0
                for (view, f, t, a), cell in result["de"].items():
                    if f == tf and t == target and a == arm and "source" in cell and (scope == "all" or cell["partition"] == scope):
                        enc += cell["encodes"] + cell["refused"]
                        fin += len(cell["finds"])
                        leg += len(cell["legacy"])
                if enc:
                    rows.append([tf, target, arm, enc, fin, f"{1000 * fin / enc:.1f}", leg, f"{1000 * leg / enc:.1f}"])
    out.append("### (b) DE and random finds (registered rule: two of three references; legacy = S and B)\n" + table(
        rows, ["family", "target", "arm", "encodes", "unique finds", "per 1,000", "legacy finds", "legacy per 1,000"]))
    rows = []
    for tf in FAMILIES:
        att = {mt: n for (p, f, mt), n in result["bytematch_cells"].items() if f == tf and (scope == "all" or p == scope)}
        total = sum(att.values())
        pairs = sum(n for (p, f, k), n in result["bm_stats"].items() if f == tf and k == "pairs" and (scope == "all" or p == scope))
        dec = sum(n for (p, f, k), n in result["bm_stats"].items() if f == tf and k == "decided" and (scope == "all" or p == scope))
        if total:
            rows.append([tf, total, att.get(True, 0), pct(att.get(True, 0), total), pairs, pct(dec, pairs)])
    out.append("### Byte-matched cells\n" + table(rows, ["family", "cells", "matched within 1%", "rate", "pairs", "decided"]))
    rows = []
    for (tf, ramp), vals in sorted(result["luma_gain"].items()):
        rows.append([tf, ramp, len(vals), f"{percentile(vals, .1):.3f}", f"{percentile(vals, .5):.3f}", f"{percentile(vals, .9):.3f}"])
    out.append("### Luma gain per trade pair (best of luma601, lumrel; dB; all partitions)\n" +
               table(rows, ["family", "ramp", "trade pairs", "p10", "p50", "p90"]))
    return "\n\n".join(out)


def probe_table(result):
    rows = []
    for tf in ("ycc444", "ycc420"):
        by_m = defaultdict(list)
        for (view, f), rs in result["probe"].items():
            if f == tf:
                for r in rs:
                    by_m[r["chroma"]].append(r["scores"])
        for m in sorted(by_m):
            sc = by_m[m]
            med = lambda k: statistics.median(x[k] for x in sc)
            rows.append([tf, m, len(sc), f"{med('ssim2-frozen'):.1f}", f"{med('butteraugli'):.2f}", f"{med('cvvdp'):.2f}",
                         f"{med('zensim-e'):.1f}", f"{med('chroma601-psnr'):.2f}", f"{med('luma601-psnr'):.2f}"])
    return "### YCbCr range probe (unit luma, chroma multiplier; medians over probe views)\n" + table(
        rows, ["family", "chroma x", "views", "S", "B", "C", "E", "chroma601 dB", "luma601 dB"])


def top_examples(result, n=3):
    out = []
    for kind in ("ladder", "bytematch"):
        for tf in FAMILIES:
            cands = [e for e in result["examples"] if e["kind"] == kind and e["family"] == tf]
            # strongest first: unanimous references, then the largest score excess of an inverted model
            cands.sort(key=lambda e: (-len(e["agreeing"]), -max(e["excess"][m] for m in e["inverted"])))
            out.extend(cands[:n])
    return out


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("runs", nargs="+", type=Path, help="run directories with ledger.jsonl")
    p.add_argument("--out", type=Path, required=True, help="new output directory")
    p.add_argument("--models", nargs="+", default=list(MODELS))
    a = p.parse_args()
    a.out.mkdir()
    result = analyse(read_rows(a.runs), a.models)
    (a.out / "TALLIES.json").write_text(json.dumps(tallies_json(result, a.models), indent=1))
    (a.out / "ladders.jsonl").write_text("".join(json.dumps(r) + "\n" for r in result["ladders"]))
    (a.out / "pairs.jsonl").write_text("".join(json.dumps(r) + "\n" for r in result["pairs"]))
    (a.out / "EXAMPLES.json").write_text(json.dumps(top_examples(result), indent=1))
    summary = dict(refused=dict(result["refused"]),
                   agree={"|".join(k): v for k, v in result["agree"].items()},
                   luma_gain={f"{k[0]}|{k[1]}": dict(n=len(v), median=statistics.median(v),
                                                     p10=percentile(v, .1), p90=percentile(v, .9))
                              for k, v in result["luma_gain"].items()},
                   de={"|".join(k): dict(encodes=c["encodes"] + c["refused"], finds=len(c["finds"]),
                                         legacy=len(c["legacy"]), partition=c.get("partition"))
                       for k, c in result["de"].items() if "source" in c},
                   bytematch_cells={"|".join(map(str, k)): v for k, v in result["bytematch_cells"].items()},
                   bm_stats={"|".join(k): v for k, v in result["bm_stats"].items()})
    (a.out / "SUMMARY.json").write_text(json.dumps(summary, indent=1))
    parts = [pilot_tables(result, a.models, scope) for scope in ("gauntlet", "training-leg", "all")]
    parts.append(probe_table(result))
    (a.out / "PILOT_TABLES.md").write_text("\n\n".join(parts) + "\n")
    print(json.dumps(dict(ladders=len(result["ladders"]), pairs=len(result["pairs"]),
                          sources=len(result["partitions"]), refused=dict(result["refused"]),
                          examples=len(result["examples"]))))


if __name__ == "__main__":
    main()
