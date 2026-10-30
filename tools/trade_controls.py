#!/usr/bin/env python3
"""Build the stored controls of a trade spec: CVVDP/butteraugli replay cells and stored
chroma-only encodes. The CVVDP doubles come from the pinned diagnostics executable; the
butteraugli doubles are the ones stored in the finished chroma-pilot containers.
"""
import argparse
import csv
import hashlib
import json
import struct
import subprocess
from pathlib import Path


def sha(data):
    return hashlib.sha256(data).hexdigest()


def chunks(data):
    if data[:12] != b"\x8bEVIF\r\n\x1a\x00\x01\x00\x00":
        raise ValueError("unexpected EVIF framing")
    offset, result = 12, {}
    while offset < len(data):
        tag = data[offset:offset + 4].decode("ascii")
        n = struct.unpack_from("<I", data, offset + 4)[0]
        offset += 8
        result[tag] = data[offset:offset + n]
        offset += n
    return result


def metric_controls(args, views):
    scratch = args.scratch
    scratch.mkdir(parents=True)
    rows, pairs = [], []
    for cell in args.cells:
        cell = Path(cell)
        view = views[cell.parent.name]
        parts = chunks(cell.read_bytes())
        scores = {(s["metric"], s.get("variant")): s["value"] for s in json.loads(parts["scor"])["scores"]}
        dist = scratch / f"{cell.parent.name}-{cell.stem}.png"
        dist.write_bytes(parts["PNGF"])
        rows.append(dict(reference=view["path"], distorted=str(dist),
                         butteraugli=scores[("butteraugli", None)],
                         butteraugli_3norm=scores[("butteraugli", "3-norm")],
                         reference_sha256=sha(Path(view["path"]).read_bytes()),
                         distorted_sha256=sha(parts["PNGF"])))
        pairs.append((view["path"], str(dist)))
    tsv = scratch / "pairs.tsv"
    with tsv.open("w") as f:
        w = csv.writer(f, delimiter="\t")
        w.writerow(["ref_path", "dist_path"])
        w.writerows(pairs)
    import pyarrow.parquet as pq
    out = scratch / "cvvdp.parquet"
    subprocess.run([str(args.diagnostics), "score-pairs", "--metric", "cvvdp", "--gpu-runtime", "cpu",
                    "--pairs-tsv", str(tsv), "--out-parquet", str(out), "--display-model", "standard_4k"],
                   check=True)
    table = pq.read_table(out).to_pylist()
    column = next(k for k in table[0] if k.startswith("cvvdp_cpu_imazen_v"))
    for row, t in zip(rows, table):
        row["cvvdp"] = float(t[column])
    return rows


def byte_controls(args):
    """Stored chroma-only cells: view, trade family, rung and the encoded SHA-256 they stored."""
    folders = {"ycc444": "ycc444", "ycc420": "ycc420", "xyb-extended": "xyb-extended", "xyb-capped": "xyb-capped"}
    result = []
    for family, folder in folders.items():
        ladders = json.loads((args.a2 / folder / "LADDERS.json").read_text())
        for ladder in ladders:
            view = Path(ladder["image"]).stem
            if view not in args.byte_views:
                continue
            for rung in args.byte_rungs:
                cell = next(c for c in ladder["cells"] if int(c["rung"]) == rung)
                result.append(dict(view=view, family=family, ramp="none", rung=rung,
                                   encoded_sha256=cell["encode_sha256"]))
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--pilot-spec", type=Path, required=True)
    p.add_argument("--diagnostics", type=Path, required=True)
    p.add_argument("--a2", type=Path, required=True)
    p.add_argument("--scratch", type=Path, required=True)
    p.add_argument("--out", type=Path, required=True)
    p.add_argument("--byte-views", nargs="+", required=True)
    p.add_argument("--byte-rungs", nargs="+", type=int, default=[3, 9])
    p.add_argument("--cells", nargs="+", required=True, help="finished chroma-pilot baseline EVIFs")
    args = p.parse_args()
    spec = json.loads(args.pilot_spec.read_text())
    views = {v["id"]: v for v in spec["views"]}
    result = dict(metric_controls=metric_controls(args, views), byte_controls=byte_controls(args),
                  diagnostics_sha256=sha(args.diagnostics.read_bytes()))
    with args.out.open("x") as f:
        json.dump(result, f, indent=2, allow_nan=False)
        f.write("\n")
    print(json.dumps({k: (len(v) if isinstance(v, list) else v) for k, v in result.items()}))


if __name__ == "__main__":
    main()
