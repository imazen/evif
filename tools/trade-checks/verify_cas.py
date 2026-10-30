#!/usr/bin/env python3
"""Verify that every DONE view of a trade run landed intact in a worker's archive CAS.

usage: verify_cas.py CAS_DIR LEDGER.parquet [--sample-evif N --evif-bin PATH]
Checks per view: result.json hash; every listed object present with a matching SHA-256; every pack
member's SHA-256; ledger row count; control receipts all pass. Optionally runs `evif verify` on a sample.
"""
import argparse, hashlib, io, json, random, subprocess, sys, tarfile, tempfile
from pathlib import Path

p = argparse.ArgumentParser()
p.add_argument("cas", type=Path)
p.add_argument("ledgers", nargs="+", type=Path)
p.add_argument("--sample-evif", type=int, default=0)
p.add_argument("--evif-bin", type=Path)
p.add_argument("--scratch", type=Path)
a = p.parse_args()

def sha(b): return hashlib.sha256(b).hexdigest()
def cas(h): return a.cas / h[:2] / h[2:4] / h

def load(path):
    if path.suffix == ".json":
        return json.loads(path.read_text())
    import pyarrow.parquet as pq
    return pq.read_table(path).to_pylist()

rows = []
for l in a.ledgers:
    rows += [r for r in load(l) if r["status"] == "done"]
summary = dict(views=len(rows), ok=0, problems=[], objects=0, members=0, bytes=0, evif_checked=0)
rng = random.Random(20261030)
for r in rows:
    view = r["image_path"].rsplit("/", 1)[-1]
    try:
        data = cas(r["output_sha"]).read_bytes()
        assert sha(data) == r["output_sha"], "result.json hash"
        m = json.loads(data)
        assert m["image_path"] == r["image_path"] and m["knob_tuple_json"] == r["knob_tuple_json"], "identity"
        by_path = {}
        for o in m["objects"]:
            blob = cas(o["sha256"]).read_bytes()
            assert sha(blob) == o["sha256"], "object hash " + o["path"]
            by_path[o["path"]] = blob
            summary["objects"] += 1
            summary["bytes"] += len(blob)
        assert sha(by_path["ledger.jsonl"]) == m["ledger_sha256"], "ledger hash"
        assert sum(1 for _ in by_path["ledger.jsonl"].splitlines()) == m["rows"], "row count"
        for name, receipt in m["controls"].items():
            assert all(x["pass"] for x in receipt), "control failed " + name
        opened = {}
        for mem in m["members"]:
            tar = opened.get(mem["pack"]) or opened.setdefault(mem["pack"], tarfile.open(fileobj=io.BytesIO(by_path[mem["pack"]])))
            content = tar.extractfile(mem["name"]).read()
            assert sha(content) == mem["sha256"] and len(content) == mem["bytes"], "member " + mem["name"]
            summary["members"] += 1
        if a.sample_evif:
            evifs = [mem for mem in m["members"] if mem["kind"] == "evif"]
            for mem in rng.sample(evifs, min(a.sample_evif, len(evifs))):
                f = Path(tempfile.mkdtemp(dir=a.scratch)) / "x.evif"
                f.write_bytes(opened[mem["pack"]].extractfile(mem["name"]).read())
                out = subprocess.run([str(a.evif_bin), "verify", str(f)], capture_output=True, text=True)
                assert out.returncode == 0 and ": ok" in out.stdout, "evif verify " + out.stdout[:200]
                summary["evif_checked"] += 1
        summary["ok"] += 1
    except Exception as e:
        summary["problems"].append(f"{view}: {e!r}")
print(json.dumps(summary, indent=1))
sys.exit(1 if summary["problems"] else 0)
