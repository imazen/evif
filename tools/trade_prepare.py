#!/usr/bin/env python3
"""Write a trade spec from the frozen chroma-pilot view list and the stored controls.

No source generation and no scheduling; the caller names the families, ramps and kinds.
`--shard I/N` keeps every N-th view so one spec can be split over processes or hosts.
"""
import argparse
import json
from pathlib import Path

FAMILIES = ["ycc444", "ycc420", "xyb-extended", "xyb-capped", "svt-420"]
RAMPS = ["none", "tiny", "medium", "large"]


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("pilot_spec", type=Path)
    p.add_argument("controls", type=Path)
    p.add_argument("output", type=Path)
    p.add_argument("--build-commit", required=True)
    p.add_argument("--seed", type=int, default=20261030)
    p.add_argument("--families", nargs="+", default=FAMILIES)
    p.add_argument("--ramps", nargs="+", default=RAMPS)
    p.add_argument("--kinds", nargs="+", default=["ladder", "bytematch", "de"])
    p.add_argument("--targets", nargs="+", default=["zensim-e", "zensim-e-codec"])
    p.add_argument("--de-samples", type=int, default=16)
    p.add_argument("--budgets", nargs="+", type=int, default=[25, 55, 85])
    p.add_argument("--map-sample-mod", type=int, default=20)
    p.add_argument("--probe-views", nargs="*", default=[])
    p.add_argument("--candidates", type=Path, help="JSON list of {label, model, sha256}")
    p.add_argument("--view-ids", nargs="*", help="keep only these views")
    p.add_argument("--limit", type=int)
    p.add_argument("--shard", default="0/1")
    a = p.parse_args()
    pilot = json.loads(a.pilot_spec.read_text())
    controls = json.loads(a.controls.read_text())
    views = pilot["views"]
    if a.view_ids:
        views = [v for v in views if v["id"] in set(a.view_ids)]
    if a.limit:
        views = views[:a.limit]
    i, n = map(int, a.shard.split("/"))
    views = [dict(v, seeds={}) for k, v in enumerate(views) if k % n == i]
    probe = [v for v in a.probe_views if v in {x["id"] for x in views}]
    spec = dict(build_commit=a.build_commit, seed=a.seed, views=views, controls=pilot["controls"],
                metric_controls=controls["metric_controls"], byte_controls=controls["byte_controls"],
                families=a.families, ramps=a.ramps, kinds=a.kinds, targets=a.targets if "de" in a.kinds else [],
                de_samples=a.de_samples, budgets=a.budgets if "bytematch" in a.kinds else [],
                map_sample_mod=a.map_sample_mod, probe_views=probe,
                candidates=json.loads(a.candidates.read_text()) if a.candidates else [])
    with a.output.open("x") as f:
        json.dump(spec, f, indent=2, allow_nan=False)
        f.write("\n")
    print(json.dumps(dict(views=len(views), families=a.families, kinds=a.kinds)))


if __name__ == "__main__":
    main()
