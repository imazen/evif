#!/usr/bin/env python3
"""Audit completed pilot ledgers and tabulate unique finds per 1,000 encodes."""
import argparse
from collections import defaultdict
import hashlib
import json
from pathlib import Path


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("run", type=Path)
    p.add_argument("--output", type=Path, help="fresh directory for a repeated read-only audit")
    a = p.parse_args()
    root = a.run
    output = root
    if a.output is not None:
        a.output.mkdir()
        output = a.output
    spec = json.loads((root / "SPEC.json").read_text())
    complete = json.loads((root / "COMPLETE.json").read_text())
    controls = json.loads((root / "CONTROL.json").read_text())
    if len(controls) != 4 or not all(c["pass"] for c in controls):
        raise ValueError("control replay missing or failed")
    expected = len(spec["views"]) * 12 * spec["samples"]
    groups = defaultdict(lambda: dict(encodes=0, unique_finds=set(), wall_seconds=0.))
    identities = {}
    partitions = defaultdict(set)
    find_sets = {"training-leg": {}, "gauntlet": {}}
    keys = set()
    for line in (root / "ledger.jsonl").read_text().splitlines():
        r = json.loads(line)
        key = (r["view"], r["family"], r["target"], r["arm"], r["index"])
        if key in keys:
            raise ValueError(f"duplicate ledger key: {key}")
        keys.add(key)
        identities.setdefault(r["source"], set()).add(r["partition"])
        partitions[r["source"]].add(r["view"])
        g = r["gains"]
        m = min(g[r["target"]] - g["ssim2-frozen"],
                g[r["target"]] - g["butteraugli"])
        if r["margin"] != m or r["find"] != (m > 0):
            raise ValueError("two-reference rule mismatch")
        if not (root / r["file"]).is_file():
            raise ValueError("candidate EVIF missing")
        if r["find"]:
            key = (r["source"], r["view"], r["encoded_sha256"])
            entry = find_sets[r["partition"]].setdefault(key, dict(
                source=r["source"], view=r["view"], encoded_sha256=r["encoded_sha256"],
                file=r["file"], target_profiles=set(), training_target="ssim2-frozen",
                frozen_teacher_score=next(s["value"] for s in r["scores"] if s["metric"] == "ssim2-frozen")))
            entry["target_profiles"].add(r["target"])
        for content in ["ALL", r["class"], "detail:" + r["content_class"]]:
            key = (r["target"], r["family"], content, r["arm"])
            groups[key]["encodes"] += 1
            if r["find"]:
                groups[key]["unique_finds"].add((r["view"], r["encoded_sha256"]))
    if len(keys) != expected or any(len(s) != 1 for s in identities.values()):
        raise ValueError("coverage or source-partition audit failed")
    classes = {v["id"]: v["class"] for v in spec["views"]}
    details = {v["id"]: "detail:" + v["content_class"] for v in spec["views"]}
    arms = {(view, family, target, arm) for view, family, target, arm, _ in keys}
    receipts = [root / view / f"{family}-{target}-{arm}.json"
                for view, family, target, arm in sorted(arms)]
    if len(receipts) != len(spec["views"]) * 12:
        raise ValueError("arm timing receipts incomplete")
    for path in receipts:
        r = json.loads(path.read_text())
        if r["encodes"] != spec["samples"]:
            raise ValueError("unequal encode budgets")
        for content in ["ALL", classes[r["view"]], details[r["view"]]]:
            groups[(r["target"], r["family"], content, r["arm"])]["wall_seconds"] += r["wall_seconds"]
    rows = []
    for (target, family, content, arm), data in sorted(groups.items()):
        n = len(data.pop("unique_finds"))
        rows.append(dict(target=target, family=family, content_class=content, arm=arm,
                         unique_finds=n, finds_per_1000=n * 1000 / data["encodes"], **data))
    result = dict(views=len(spec["views"]), sources=len(identities),
                  baseline_encodes=len(spec["views"]) * 3 * 21,
                  search_encodes=expected, wall_seconds=complete["wall_seconds"], rows=rows,
                  ledger_sha256=hashlib.sha256((root / "ledger.jsonl").read_bytes()).hexdigest())
    for partition, entries in find_sets.items():
        with (output / f"{partition.upper()}.jsonl").open("x") as f:
            for _, entry in sorted(entries.items()):
                entry["target_profiles"] = sorted(entry["target_profiles"])
                f.write(json.dumps(entry, allow_nan=False) + "\n")
    reservation = {source: dict(partition=next(iter(identities[source])), views=sorted(views))
                   for source, views in sorted(partitions.items())}
    with (output / "PARTITIONS.json").open("x") as f:
        json.dump(reservation, f, indent=2)
        f.write("\n")
    with (output / "SUMMARY.json").open("x") as f:
        json.dump(result, f, indent=2, allow_nan=False)
        f.write("\n")
    lines = ["| Target | Family | Class | Arm | Encodes | Unique finds | Finds/1,000 | Wall seconds |",
             "|---|---|---|---|---:|---:|---:|---:|"]
    lines.extend(f'| {r["target"]} | {r["family"]} | {r["content_class"]} | {r["arm"]} | '
                 f'{r["encodes"]} | {r["unique_finds"]} | {r["finds_per_1000"]:.6f} | {r["wall_seconds"]:.6f} |'
                 for r in rows)
    with (output / "SUMMARY.md").open("x") as f:
        f.write("\n".join(lines) + "\n")
    print(json.dumps({k: v for k, v in result.items() if k != "rows"}, indent=2))


if __name__ == "__main__":
    main()
