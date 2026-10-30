#!/usr/bin/env python3
"""Sample EVIFs out of the grid packs: parse each, compare its encoded bytes and every score bit for bit with the
ledger row, and run `evif verify`.
usage: sample_evif_verify.py CAS_DIR ASSEMBLED_DIR EVIF_BIN N_JOBS PER_JOB SCRATCH > receipt.json
"""
import hashlib, io, json, random, struct, subprocess, sys, tarfile, tempfile
from pathlib import Path

cas, assembled, evif, n_jobs, per_job, scratch = Path(sys.argv[1]), Path(sys.argv[2]), Path(sys.argv[3]), int(sys.argv[4]), int(sys.argv[5]), Path(sys.argv[6])
rng = random.Random(20261030)


def sha(b):
    return hashlib.sha256(b).hexdigest()


def obj(h):
    return (cas / h[:2] / h[2:4] / h).read_bytes()


def chunks(d):
    off, out = 12, {}
    while off < len(d):
        tag = d[off:off + 4].decode()
        n = struct.unpack_from("<I", d, off + 4)[0]
        off += 8
        out[tag] = d[off:off + n]
        off += n
    return out


jobs = sorted(p for p in assembled.iterdir() if p.is_dir())
picked = rng.sample(jobs, min(n_jobs, len(jobs)))
summary = dict(jobs=0, evifs=0, bits_mismatch=0, score_mismatch=0, verify_failed=0, problems=[])
for jd in picked:
    job = json.loads((jd / "JOB.json").read_text())
    result = json.loads(obj(job["output_sha"]))
    rows = {}
    for line in (jd / "ledger.jsonl").open():
        r = json.loads(line)
        if "file" in r:
            rows[r["file"]] = r
    evif_members = [m for m in result["members"] if m["kind"] == "evif"]
    if not evif_members:
        continue
    summary["jobs"] += 1
    objects = {o["path"]: o["sha256"] for o in result["objects"]}
    for m in rng.sample(evif_members, min(per_job, len(evif_members))):
        try:
            tar = tarfile.open(fileobj=io.BytesIO(obj(objects[m["pack"]])))
            data = tar.extractfile(m["name"]).read()
            assert sha(data) == m["sha256"], "member hash"
            c = chunks(data)
            row = rows[m["name"]]
            if sha(c["BITS"][4:]) != row["encoded_sha256"]:
                summary["bits_mismatch"] += 1
                summary["problems"].append(f"bits {jd.name}/{m['name']}")
                continue
            scores = {(s["metric"] if not s.get("variant") else s["metric"] + "/" + s["variant"]): s["value"]
                      for s in json.loads(c["scor"])["scores"]}
            bad = [k for k, v in row["scores"].items() if k in scores and scores[k] != v]
            if bad:
                summary["score_mismatch"] += 1
                summary["problems"].append(f"scores {jd.name}/{m['name']}: {bad[:3]}")
                continue
            f = Path(tempfile.mkdtemp(dir=scratch)) / "x.evif"
            f.write_bytes(data)
            out = subprocess.run([str(evif), "verify", str(f)], capture_output=True, text=True)
            if out.returncode != 0 or ": ok" not in out.stdout:
                summary["verify_failed"] += 1
                summary["problems"].append(f"verify {jd.name}/{m['name']}: {out.stdout[:100]}")
            summary["evifs"] += 1
        except Exception as e:
            summary["problems"].append(f"{jd.name}/{m['name']}: {e!r}")
print(json.dumps(summary, indent=1))
