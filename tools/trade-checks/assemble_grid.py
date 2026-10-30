#!/usr/bin/env python3
"""Pull the per-job ledgers of a finished (or running) trade jobset out of the archive CAS.

usage: assemble_grid.py CAS_DIR OUT_DIR LEDGER.parquet [LEDGER.parquet ...]
For every DONE job: read its result.json from the CAS (hash-checked), read the ledger object it names
(hash-checked), and write OUT_DIR/<view>__<template-key>/ledger.jsonl plus the receipts of the run
(SPEC/JUDGES/CONTROL/... from the same job). A view declared as several family jobs gets one directory
per job; `trade_gate.py` concatenates them (family rows are disjoint; baseline rows are ignored there).
Writes OUT_DIR/ASSEMBLY.json with counts and every problem. Exits 1 when any job fails a check.
"""
import argparse, hashlib, json, sys
from pathlib import Path

p = argparse.ArgumentParser(description=__doc__)
p.add_argument("cas", type=Path)
p.add_argument("out", type=Path)
p.add_argument("ledgers", nargs="+", type=Path)
a = p.parse_args()


def sha(b): return hashlib.sha256(b).hexdigest()
def cas(h): return a.cas / h[:2] / h[2:4] / h


def load(path):
    if path.suffix == ".json":
        return json.loads(path.read_text())
    import pyarrow.parquet as pq
    return pq.read_table(path).to_pylist()


rows = {}
for l in a.ledgers:
    for r in load(l):
        if r["status"] == "done":
            rows[r["job_id"]] = r          # a job may be re-run after a failure: the done row wins
a.out.mkdir(parents=True, exist_ok=True)
summary = dict(jobs=len(rows), ok=0, ledger_rows=0, problems=[], by_template={})
for r in sorted(rows.values(), key=lambda r: r["image_path"]):
    knobs = json.loads(r["knob_tuple_json"])
    view = knobs["view"]["id"]
    template = Path(knobs["template_path"]).stem.removeprefix("template-")
    try:
        data = cas(r["output_sha"]).read_bytes()
        assert sha(data) == r["output_sha"], "result.json hash"
        m = json.loads(data)
        assert m["knob_tuple_json"] == r["knob_tuple_json"], "identity"
        objects = {o["path"]: o["sha256"] for o in m["objects"]}
        ledger = cas(objects["ledger.jsonl"]).read_bytes()
        assert sha(ledger) == m["ledger_sha256"] == objects["ledger.jsonl"], "ledger hash"
        assert sum(1 for _ in ledger.splitlines()) == m["rows"], "row count"
        d = a.out / f"{view}__{template}"
        d.mkdir(exist_ok=False)
        (d / "ledger.jsonl").write_bytes(ledger)
        for name in ("SPEC.json", "JUDGES.json", "SOURCE_IDENTITIES.json", "CONTROL.json", "METRIC_CONTROL.json",
                     "BYTE_CONTROL.json", "COMPLETE.json", "timing.json"):
            if name in objects:
                blob = cas(objects[name]).read_bytes()
                assert sha(blob) == objects[name], "receipt hash " + name
                (d / name).write_bytes(blob)
        (d / "JOB.json").write_text(json.dumps(dict(job_id=r["job_id"], output_sha=r["output_sha"], view=view,
                                                    template=template, rows=m["rows"], worker=r["worker"],
                                                    members=len(m["members"]), objects=len(m["objects"])), indent=1))
        summary["ok"] += 1
        summary["ledger_rows"] += m["rows"]
        summary["by_template"][template] = summary["by_template"].get(template, 0) + 1
    except Exception as e:
        summary["problems"].append(f"{view}__{template}: {e!r}")
(a.out / "ASSEMBLY.json").write_text(json.dumps(summary, indent=1))
print(json.dumps({k: v for k, v in summary.items() if k != "problems"} | {"problems": len(summary["problems"])}))
for line in summary["problems"][:20]:
    print("PROBLEM", line)
sys.exit(1 if summary["problems"] else 0)
