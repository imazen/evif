#!/usr/bin/env python3
"""Bind the existing TRAIN pilot views and admitted ladder seeds into a search spec.

No source generation, rating ingestion, or fleet scheduling. The caller supplies
the existing chroma study root and a fresh output file. Paths remain explicit.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path


def read(path):
    return json.loads(path.read_text())


def seeds(ladder, family):
    kept = ladder["admission"]["kept_indices"]
    # No admitted ladder means no imported seed; the full budget is still run.
    selected = [kept[0], kept[-1]] if kept else []
    result = []
    for i in selected:
        p = json.loads(ladder["cells"][i]["params"])
        if family == "zenjpeg-color":
            x = [(math.log(s) - math.log(.1)) / math.log(50)
                 for s in p["tables"]["chroma_scales"]]
        else:
            # Zero-table rungs lie outside the logarithmic search space.
            scales = p["quant_scale"][1] + p["quant_scale"][2]
            if any(s <= 0 for s in scales):
                continue
            x = [(math.log2(s) + 6) / 12 for s in scales]
        if any(v < -1e-14 or v > 1 + 1e-14 for v in x):
            raise ValueError("admitted rung outside declared chroma bounds")
        result.append([min(1., max(0., v)) for v in x])
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("study", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--build-commit", required=True)
    parser.add_argument("--samples", type=int, default=16)
    parser.add_argument("--seed", type=int, default=20261029)
    a = parser.parse_args()
    prepared = read(a.study / "pilot-run04/PREPARED.json")
    source_spec = read(a.study / "pilot-run04/SOURCES.json")
    ladders = {}
    for family, folder in [("zenjpeg-expert", "ycc444"),
                           ("zenjpeg-color", "xyb-capped")]:
        ladders[family] = {Path(x["image"]).stem: x
                           for x in read(a.study / "a2" / folder / "LADDERS.json")}
    views = []
    for p in prepared:
        if p["split"].lower() != "train":
            raise ValueError("non-TRAIN source in pilot receipt")
        name = Path(p["path"]).name
        path = a.study / "a2/source-views" / name
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        if digest != p["sha256"]:
            raise ValueError(f"source view differs: {path}")
        view = dict(id=path.stem, source=p["reference_group"],
                    source_path=str(a.study / "raw" / p["raw"]),
                    source_sha256=p["source_sha256"],
                    **{"class": p["class_group"], "content_class": p["content_class"]}, split="train", path=str(path),
                    sha256=digest, seeds={})
        for family, by_view in ladders.items():
            view["seeds"][family] = seeds(by_view[path.stem], family)
        # There are no admitted SDR SVT ladders in A2. Do not relabel HDR seeds.
        view["seeds"]["zenav1-svt"] = []
        views.append(view)
    if len(views) != 162 or len({v["id"] for v in views}) != 162:
        raise ValueError("the frozen pilot must have exactly 162 distinct views")
    controls = []
    for i, c in enumerate(source_spec["controls"]):
        controls.append(dict(reference=str(a.study / "raw" / c["raw"]),
                             distorted=str(a.study / "a2/judge-replay" / f"control{i}.png"),
                             expected=c["expected_ssim2"],
                             reference_pixels_sha256=c["reference_pixels_sha256"],
                             distorted_pixels_sha256=c["distorted_pixels_sha256"]))
    spec = dict(build_commit=a.build_commit, samples=a.samples, seed=a.seed,
                views=views, controls=controls)
    with a.output.open("x") as f:
        json.dump(spec, f, indent=2, allow_nan=False)
        f.write("\n")


if __name__ == "__main__":
    main()
