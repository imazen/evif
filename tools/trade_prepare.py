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
    p.add_argument("--persist-training-leg", choices=["evif", "bitstream"], default="evif")
    p.add_argument("--no-find-maps", action="store_true")
    p.add_argument("--shard", default="0/1")
    p.add_argument("--balanced-shards", type=int, help="write OUTPUT-shard-NN.json, largest views first")
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
    def spec_for(vs):
        keep = {x["id"] for x in vs}
        return dict(build_commit=a.build_commit, seed=a.seed, views=vs, controls=pilot["controls"],
                    metric_controls=controls["metric_controls"],
                    byte_controls=[c for c in controls["byte_controls"] if c["family"] in a.families],
                    families=a.families, ramps=a.ramps, kinds=a.kinds,
                    targets=a.targets if "de" in a.kinds else [],
                    de_samples=a.de_samples, budgets=a.budgets if "bytematch" in a.kinds else [],
                    map_sample_mod=a.map_sample_mod, probe_views=[v for v in a.probe_views if v in keep],
                    candidates=json.loads(a.candidates.read_text()) if a.candidates else [],
                    persist_training_leg=a.persist_training_leg, find_maps=not a.no_find_maps)

    def write(path, spec):
        with path.open("x") as f:
            json.dump(spec, f, indent=2, allow_nan=False)
            f.write("\n")

    if a.balanced_shards:
        def area(v):
            header = Path(v["path"]).read_bytes()[16:24]
            return int.from_bytes(header[:4], "big") * int.from_bytes(header[4:], "big")
        # Longest-processing-time first: each view goes to the lightest shard.
        shards = [[] for _ in range(a.balanced_shards)]
        load = [0] * a.balanced_shards
        for v in sorted(views, key=lambda v: (-area(v), v["id"])):
            k = load.index(min(load))
            shards[k].append(v)
            load[k] += area(v)
        for k, vs in enumerate(shards):
            write(a.output.with_name(f"{a.output.stem}-shard-{k:02d}.json"), spec_for(vs))
        print(json.dumps(dict(shards=a.balanced_shards, views=[len(x) for x in shards], load=load)))
        return
    write(a.output, spec_for(views))
    print(json.dumps(dict(views=len(views), families=a.families, kinds=a.kinds)))


if __name__ == "__main__":
    main()
