#!/usr/bin/env python3
"""Ledger -> truth votes, inversions, finds and the per-source tallies of G-TRADE.

Definitions are the registered ones in docs/trade-ladders.md (and its amendment). Nothing here
fits or tunes anything. The paired bootstrap over sources is `evif trade-bootstrap`
(zenmetrics-stats); this tool writes its input (`TALLIES.json`) and the descriptive tables.
"""
import argparse
import itertools
import json
import re
import statistics
from collections import Counter, defaultdict
from pathlib import Path

REFS = ("ssim2-frozen", "butteraugli", "cvvdp")
LOWER_IS_BETTER = {"butteraugli"}
MARGIN = {"ssim2-frozen": 1.0, "butteraugli": 0.5, "cvvdp": 0.25}
MODELS = ("zensim-e", "zensim-e-codec", "zensim-f", "zensim-f-codec")
DIAG_STEP = 0.02  # dB, amended trade-pair criterion
FAMILIES = ("ycc444", "ycc420", "xyb-extended", "xyb-capped", "svt-420")
# Amendment 2026-10-30: SSIMULACRA2 is not valid on frames with a long edge of 64 px or less. On those
# views it does not vote, finds need both butteraugli and CVVDP, and the gate is reported per stratum.
SMALL_EDGE = 64
RULE_SMALL = ("butteraugli", "cvvdp")


class Sizes:
    """Long edge per view: an explicit table (view -> [width, height]) when given, else the cap in the view
    id (`..._m64` is a view whose long edge is 64; the sources are larger than every cap)."""

    def __init__(self, dims=None):
        self.dims = dims or {}

    def long_edge(self, view):
        if view in self.dims:
            return max(self.dims[view])
        m = re.search(r"_m(\d+)$", view)
        return int(m.group(1)) if m else None

    def small(self, view):
        edge = self.long_edge(view)
        return edge is not None and edge <= SMALL_EDGE


def scopes_of(partition, small):
    """Every report scope a row belongs to: its partition, its partition and size stratum, and the same for all."""
    size = "small" if small else "regular"
    return (partition, f"{partition}/{size}", "all", f"all/{size}")


def split_scope(scope):
    part, _, size = scope.partition("/")
    return part, (size or None)


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


def truth(v, small=False):
    """WORSE/BETTER when at least two distinct references agree, else UNDECIDED. On a view of
    SMALL_EDGE px or less SSIMULACRA2 does not vote, so butteraugli and CVVDP must both agree."""
    if small:
        v = {r: x for r, x in v.items() if r != "ssim2-frozen"}
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


def analyse(rows, models, sizes=None):
    """Walk the ledger once; return tallies and the descriptive tables."""
    sizes = sizes or Sizes()
    ladders = defaultdict(list)
    bytematch = defaultdict(list)
    de = defaultdict(lambda: dict(encodes=0, refused=0, finds=set(), legacy=set(), strong05=set(), strong10=set()))
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
        small = sizes.small(r["view"])
        if kind == "bytematch":
            for scope in scopes_of(r["partition"], small):
                bytematch_cells[(scope, r["trade_family"], bool(r.get("matched")))] += 1
        if kind == "ladder":
            ladders[(r["view"], r["trade_family"], r["ramp"])].append(r)
        elif kind == "bytematch" and r.get("matched"):
            bytematch[(r["view"], r["trade_family"], r["budget_quality"])].append(r)
        elif kind == "de":
            cell = de[(r["view"], r["trade_family"], r["target"], r["arm"])]
            cell["encodes"] += 1
            cell["source"] = r["source"]
            cell["partition"] = r["partition"]
            cell["small"] = small
            if small:
                # Both valid references must show a positive margin; the two-reference legacy find has no
                # meaning without SSIMULACRA2.
                m = r["margins"]
                margin = min(m["butteraugli"], m["cvvdp"])
                find, legacy = margin > 0, False
            else:
                margin, find, legacy = r["margin"], r["find"], r["legacy_find"]
            if find:
                cell["finds"].add(r["encoded_sha256"])
            if legacy:
                cell["legacy"].add(r["encoded_sha256"])
            if margin > 0.5:
                cell["strong05"].add(r["encoded_sha256"])
            if margin > 1.0:
                cell["strong10"].add(r["encoded_sha256"])
        elif kind == "probe":
            probe[(r["view"], r["trade_family"])].append(r)

    metrics = ("a", "a2", "b", "a_s2", "a_s4", "a_s8")
    tally = {k: Tally() for k in metrics}                       # pooled: each view under its own rule
    tally_by_size = {z: {k: Tally() for k in metrics} for z in ("regular", "small")}

    def add(metric, small, source, stratum, model, **counts):
        tally[metric].add(source, stratum, model, **counts)
        tally_by_size["small" if small else "regular"][metric].add(source, stratum, model, **counts)

    agree = Counter()
    examples = []
    bm_stats = Counter()
    luma_gain = defaultdict(list)
    ladder_rows = []
    pair_rows = []
    for (view, tf, ramp), rs in sorted(ladders.items()):
        rs.sort(key=lambda r: r["rung"])
        source = rs[0]["source"]
        small = sizes.small(view)
        rule_refs = RULE_SMALL if small else REFS
        trade_pairs = 0
        worse_pairs = 0
        inverted_pairs = Counter()
        scores_s = [r["scores"]["ssim2-frozen"] for r in rs]
        kept, stop = admission(scores_s)
        for a, b in zip(rs, rs[1:]):
            criterion, deltas = trade_pair(a, b)
            is_trade = ramp != "none" and criterion
            v = votes(a["scores"], b["scores"])
            t, agreeing = truth(v, small)
            flags = model_inversions(a["scores"], b["scores"], t, models)
            pair_rows.append(dict(view=view, source=source, partition=a["partition"], family=tf, ramp=ramp,
                                  small=small, rung_a=a["rung"], rung_b=b["rung"], trade=is_trade, truth=t, votes=v,
                                  **deltas))
            if not is_trade:
                continue
            trade_pairs += 1
            worse_pairs += t == "WORSE"
            for m, f in flags.items():
                inverted_pairs[m] += f == "inversion"
            luma_gain[(tf, ramp)].append(max(deltas["luma601"], deltas["lumrel"]))
            part = a["partition"]
            for scope in scopes_of(part, small):
                agree[(scope, tf, "trade-pairs")] += 1
                agree[(scope, tf, "decided")] += t != "UNDECIDED"
                agree[(scope, tf, "decided-worse")] += t == "WORSE"
                agree[(scope, tf, "unanimous")] += t != "UNDECIDED" and len(agreeing) == len(rule_refs)
                for r in rule_refs:
                    if t != "UNDECIDED" and r not in agreeing:
                        agree[(scope, tf, "dissent:" + r)] += 1
                if t == "WORSE" and not small and v["ssim2-frozen"] == "better":
                    agree[(scope, tf, "ssim2-says-better-while-others-say-worse")] += 1
            if t == "WORSE" and any(f == "inversion" for f in flags.values()):
                examples.append(dict(kind="ladder", view=view, partition=part, small=small, family=tf, ramp=ramp, rung_a=a["rung"],
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
                    add("a", small, source, tf, m, pairs=1, undecided=int(t == "UNDECIDED"),
                        n_worse=int(t == "WORSE"), inv=int(f == "inversion"),
                        n_better=int(t == "BETTER"), reverse=int(f == "reverse"))
        # Exploratory strides (not the registered gate): the same trade-pair criterion and vote rule
        # between rungs two, four and eight apart, where changes clear the vote margins more often.
        for stride in (2, 4, 8):
            for a, b in zip(rs, rs[stride:]):
                criterion, deltas = trade_pair(a, b)
                if ramp == "none" or not criterion:
                    continue
                t, _ = truth(votes(a["scores"], b["scores"]), small)
                flags = model_inversions(a["scores"], b["scores"], t, models)
                for m in models:
                    if m in a["scores"]:
                        f = flags.get(m)
                        add(f"a_s{stride}", small, source, tf, m, pairs=1, undecided=int(t == "UNDECIDED"),
                            n_worse=int(t == "WORSE"), inv=int(f == "inversion"),
                            n_better=int(t == "BETTER"), reverse=int(f == "reverse"))
        first, last = rs[0], rs[-1]
        ladder_rows.append(dict(view=view, source=source, partition=rs[0]["partition"], small=small, family=tf, ramp=ramp,
                                rungs=len(rs), trade_pairs=trade_pairs, admitted=trade_pairs >= 3, kept=len(kept),
                                worse_pairs=worse_pairs, inverted_pairs=dict(inverted_pairs),
                                stop=stop, depth_past_stop=(len(rs) - 1 - stop) if stop is not None else 0,
                                chroma_at_stop=rs[stop]["chroma"] if stop is not None else None,
                                chroma_last=last["chroma"],
                                luma601_gain=last["scores"]["luma601-psnr"] - first["scores"]["luma601-psnr"],
                                lumrel_gain=last["scores"]["lumrel-psnr"] - first["scores"]["lumrel-psnr"],
                                chroma601_loss=first["scores"]["chroma601-psnr"] - last["scores"]["chroma601-psnr"]))
    for (view, tf, budget), rs in sorted(bytematch.items()):
        rs.sort(key=lambda r: r["rho"])
        source = rs[0]["source"]
        small = sizes.small(view)
        for a, b in itertools.combinations(rs, 2):
            v = votes(a["scores"], b["scores"])
            t, agreeing = truth(v, small)
            flags = model_inversions(a["scores"], b["scores"], t, models)
            for scope in scopes_of(a["partition"], small):
                bm_stats[(scope, tf, "pairs")] += 1
                bm_stats[(scope, tf, "decided")] += t != "UNDECIDED"
            if t == "WORSE" and any(f == "inversion" for f in flags.values()):
                examples.append(dict(kind="bytematch", view=view, partition=a["partition"], small=small, family=tf,
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
                    add("a2", small, source, tf, m, pairs=1, n_worse=int(t == "WORSE"), inv=int(f == "inversion"),
                        n_better=int(t == "BETTER"), reverse=int(f == "reverse"),
                        undecided=int(t == "UNDECIDED"))
    for (view, tf, target, arm), cell in de.items():
        if "source" not in cell:
            continue
        budget = cell["encodes"] + cell["refused"]
        add("b", cell["small"], cell["source"], f"{tf}/{arm}", target, encodes=budget, finds=len(cell["finds"]),
            legacy_finds=len(cell["legacy"]))
    return dict(tally=tally, tally_by_size=tally_by_size, sizes=sizes, partitions=partitions, agree=agree, luma_gain=luma_gain, ladders=ladder_rows,
                pairs=pair_rows, probe=probe, refused=refused, examples=examples, de=de, bm_stats=bm_stats,
                bytematch_cells=bytematch_cells)


def tallies_json(result, models):
    """Per partition and gate metric: model -> source -> counts, pooled over strata. Keys are the three
    partitions (each view under its own truth rule) and, where views of that size exist, `<partition>/regular`
    (long edge above 64 px) and `<partition>/small` (64 px or less)."""
    out = {}
    sets = (("", result["tally"]), ("/regular", result["tally_by_size"]["regular"]),
            ("/small", result["tally_by_size"]["small"]))
    for part in ("gauntlet", "training-leg", "all"):
        for suffix, tallies in sets:
            block = {}
            for metric, tally in tallies.items():
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
                    block.setdefault(key_metric, {})[model] = {s: dict(c) for s, c in by_source.items()}
            if block or not suffix:
                out[part + suffix] = block
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
    """Descriptive pilot statistics (markdown) for one scope: a partition name or 'all', optionally with a
    size stratum (`gauntlet/small`, `gauntlet/regular`)."""
    part = result["partitions"]
    scope_part, scope_size = split_scope(scope)
    tallies = result["tally_by_size"][scope_size] if scope_size else result["tally"]
    rule = {"small": "views of 64 px or less: butteraugli and CVVDP must both agree (SSIMULACRA2 does not vote)",
            "regular": "views above 64 px: two of three references"}.get(scope_size, "each view under its own rule")
    out = [f"## Scope: {scope}\n\nTruth rule: {rule}."]
    lad = [r for r in result["ladders"]
           if (scope_part == "all" or r["partition"] == scope_part)
           and (scope_size is None or r["small"] == (scope_size == "small"))]
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
        ls = [r for r in lad if r["family"] == tf and r["ramp"] != "none"]
        if not ls:
            continue
        with_worse = [r for r in ls if r["worse_pairs"] > 0]
        admitted = [r for r in ls if r["admitted"]]
        row = [tf, len(ls), len(admitted), len(with_worse)]
        for m in models:
            hit = sum(1 for r in with_worse if r["inverted_pairs"].get(m, 0) > 0)
            row.append(f"{hit} ({pct(hit, len(with_worse))})")
        rows.append(row)
    out.append("### Ladders on which a model inverts (ramps other than none; share of ladders with a decided-WORSE trade pair)\n" +
               table(rows, ["family", "ladders", "admitted", "with a decided-WORSE trade pair"] +
                     [f"{m} inverts" for m in models]))
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
                c = sum_tally(tallies[metric], scope_part, part, m).get(tf)
                if c and c["pairs"]:
                    rows.append([tf, m, c["pairs"], c["undecided"], c["n_worse"], c["inv"], pct(c["inv"], c["n_worse"]),
                                 c["n_better"], c["reverse"], pct(c["reverse"], c["n_better"])])
        out.append(f"### {label}\n" + table(rows, ["family", "model", "pairs", "undecided", "decided WORSE",
                                                      "inversions", "rate", "decided BETTER", "reverse", "rate"]))
    rows = []
    for tf in FAMILIES:
        for target in models:
            for arm in ("de", "random"):
                enc = fin = leg = s05 = s10 = 0
                for (view, f, t, a), cell in result["de"].items():
                    if f == tf and t == target and a == arm and "source" in cell and scope in scopes_of(cell["partition"], cell["small"]):
                        enc += cell["encodes"] + cell["refused"]
                        fin += len(cell["finds"])
                        leg += len(cell["legacy"])
                        s05 += len(cell["strong05"])
                        s10 += len(cell["strong10"])
                if enc:
                    rows.append([tf, target, arm, enc, fin, f"{1000 * fin / enc:.1f}", leg, f"{1000 * leg / enc:.1f}",
                                 f"{1000 * s05 / enc:.1f}", f"{1000 * s10 / enc:.1f}"])
    out.append("### (b) DE and random finds (registered rule: two of three references; legacy = S and B; strong = margin above 0.5 / 1.0 log2 bytes)\n" + table(
        rows, ["family", "target", "arm", "encodes", "unique finds", "per 1,000", "legacy finds", "legacy per 1,000",
               "strong>0.5 per 1,000", "strong>1.0 per 1,000"]))
    rows = []
    for tf in FAMILIES:
        att = {mt: n for (p, f, mt), n in result["bytematch_cells"].items() if f == tf and p == scope}
        total = sum(att.values())
        pairs = sum(n for (p, f, k), n in result["bm_stats"].items() if f == tf and k == "pairs" and p == scope)
        dec = sum(n for (p, f, k), n in result["bm_stats"].items() if f == tf and k == "decided" and p == scope)
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
    p.add_argument("--dims", type=Path, help="JSON {view: [width, height]}; else the cap in the view id decides")
    a = p.parse_args()
    a.out.mkdir()
    sizes = Sizes(json.loads(a.dims.read_text()) if a.dims else None)
    result = analyse(read_rows(a.runs), a.models, sizes)
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
                                         legacy=len(c["legacy"]), strong05=len(c["strong05"]),
                                         strong10=len(c["strong10"]), partition=c.get("partition"))
                       for k, c in result["de"].items() if "source" in c},
                   bytematch_cells={"|".join(map(str, k)): v for k, v in result["bytematch_cells"].items()},
                   bm_stats={"|".join(k): v for k, v in result["bm_stats"].items()})
    (a.out / "SUMMARY.json").write_text(json.dumps(summary, indent=1))
    scopes = []
    for part_name in ("gauntlet", "training-leg", "all"):
        scopes.append(part_name)
        for size in ("regular", "small"):
            if any(k[0] == f"{part_name}/{size}" for k in result["agree"]) or \
                    any(r["small"] == (size == "small") and (part_name == "all" or r["partition"] == part_name)
                        for r in result["ladders"]):
                scopes.append(f"{part_name}/{size}")
    parts = [pilot_tables(result, a.models, scope) for scope in scopes]
    parts.append(probe_table(result))
    (a.out / "PILOT_TABLES.md").write_text("\n\n".join(parts) + "\n")
    print(json.dumps(dict(ladders=len(result["ladders"]), pairs=len(result["pairs"]),
                          sources=len(result["partitions"]), refused=dict(result["refused"]),
                          examples=len(result["examples"]))))


if __name__ == "__main__":
    main()
