#!/usr/bin/env python3
"""Audit finished trade run directories: every ledger row's file exists; a sample of EVIFs is parsed and
compared with its row (encoded bytes SHA-256, every score bit-for-bit) and passes `evif verify`.
usage: audit_pilot.py EVIF_BIN SAMPLE_FRACTION RUN_DIR... > receipt.json"""
import hashlib, json, random, struct, subprocess, sys, tempfile
from pathlib import Path

evif, frac, runs = Path(sys.argv[1]), float(sys.argv[2]), [Path(r) for r in sys.argv[3:]]
rng = random.Random(20261030)
def sha(b): return hashlib.sha256(b).hexdigest()
def chunks(d):
    off, out = 12, {}
    while off < len(d):
        tag = d[off:off + 4].decode(); n = struct.unpack_from("<I", d, off + 4)[0]; off += 8
        out[tag] = d[off:off + n]; off += n
    return out
total = Counter = dict(rows=0, missing=0, empty=0, sampled=0, mismatched=0, verify_failed=0, refused=0, shards=0)
problems = []
for run in runs:
    total["shards"] += 1
    for line in (run / "ledger.jsonl").open():
        r = json.loads(line)
        total["rows"] += 1
        if "refused" in r:
            total["refused"] += 1
            continue
        f = run / r["file"]
        if not f.exists():
            total["missing"] += 1; problems.append(f"missing {f}"); continue
        if f.stat().st_size == 0:
            total["empty"] += 1; continue
        if f.suffix == ".evif" and rng.random() < frac:
            total["sampled"] += 1
            d = f.read_bytes()
            c = chunks(d)
            if sha(c["BITS"][4:]) != r["encoded_sha256"]:
                total["mismatched"] += 1; problems.append(f"bits sha {f}"); continue
            scores = {(s["metric"] if "variant" not in s or not s.get("variant") else s["metric"] + "/" + s["variant"]): s["value"]
                      for s in json.loads(c["scor"])["scores"]}
            for k, v in r["scores"].items():
                if struct.pack("<d", scores[k]) != struct.pack("<d", v):
                    total["mismatched"] += 1; problems.append(f"score {k} {f}"); break
            t = Path(tempfile.mkdtemp(dir=Path.home() / "tmp")) / "x.evif"
            t.write_bytes(d)
            p = subprocess.run([str(evif), "verify", str(t)], capture_output=True, text=True)
            if p.returncode:
                total["verify_failed"] += 1; problems.append(f"verify {f}: {p.stdout[:120]}")
            t.unlink(); t.parent.rmdir()
        elif f.suffix != ".evif" and sha(f.read_bytes()) != r["encoded_sha256"] and rng.random() < frac:
            total["mismatched"] += 1; problems.append(f"bitstream sha {f}")
print(json.dumps(dict(totals=total, problems=problems[:50]), indent=1))
sys.exit(1 if problems else 0)
