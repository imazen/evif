#!/usr/bin/env python3
"""Independent legacy framing, FFmpeg carrier decode, score and map binding audit.

FFmpeg only reads bytes. No generated image is rewritten with an oracle.
The native EVIF verifier separately checks SEAL and pixel digests.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import os
import hashlib
import json
from pathlib import Path
import struct
import subprocess


def sha(data):
    return hashlib.sha256(data).hexdigest()


def chunks(data):
    if data[:12] != b"\x8bEVIF\r\n\x1a\x00\x01\x00\x00":
        raise ValueError("unexpected legacy EVIF header")
    result = {}
    pos = 12
    while pos < len(data):
        tag, size = struct.unpack_from("<4sI", data, pos)
        start = pos + 8
        if tag in result or start + size > len(data):
            raise ValueError("duplicate/truncated chunk")
        result[tag] = data[start:start+size]
        pos = start + size
    if pos != len(data) or b"SEAL" not in result:
        raise ValueError("truncated/unsealed container")
    return result


def decode(binary, data):
    cmd = [str(binary), "-nostdin", "-v", "error", "-threads", "1", "-i", "pipe:0",
           "-frames:v", "1", "-threads", "1", "-f", "rawvideo", "-pix_fmt", "rgb24", "pipe:1"]
    r = subprocess.run(cmd, input=data, capture_output=True, check=False)
    if r.returncode:
        raise ValueError(r.stderr.decode(errors="replace"))
    return r.stdout


def verify_one(path, root, ffmpeg, references, ledger):
    container = path.read_bytes()
    c = chunks(container)
    width, height = struct.unpack_from("<II", c[b"HEAD"])
    if c[b"HEAD"][8:10] != bytes([3, 1]):
        raise ValueError("pilot rendition must be RGB8")
    if width * height >= 1_000_000:
        raise ValueError("pilot exceeded the one-megapixel cap")
    bits = c[b"BITS"][4:]
    digest = sha(bits)
    rendition = decode(ffmpeg, c[b"PNGF"])
    if len(rendition) != width * height * 3:
        raise ValueError("rendition geometry differs")
    decoded = decode(ffmpeg, bits)
    if len(decoded) != len(rendition):
        raise ValueError("candidate bitstream geometry differs")
    stored = json.loads(c[b"scor"])
    scores = stored["scores"]
    for s in scores:
        if s["metric"] == "butteraugli":
            m = s["diffmap"]
            payload = (root / m["path"]).read_bytes()
            if (sha(payload) != m["sha256"] or sha(rendition) != m["distorted_pixels_sha256"]
                    or references[path.parent.name] != m["reference_pixels_sha256"]):
                raise ValueError("map/rendition hash binding differs")
            if (m["width"], m["height"]) != (width, height):
                raise ValueError("map geometry differs")
            head = f"Pf\n{width} {height}\n-1.0\n".encode()
            if not payload.startswith(head) or len(payload) != len(head) + width * height * 4:
                raise ValueError("PFM shape/length differs")
    relative = str(path.relative_to(root))
    if relative in ledger:
        r = ledger[relative]
        if r["encoded_sha256"] != digest or len(scores) != len(r["scores"]):
            raise ValueError("ledger/EVIF identity differs")
        for left, right in zip(scores, r["scores"]):
            if left != right or struct.pack("<d", left["value"]) != struct.pack("<d", right["value"]):
                raise ValueError("score double/provenance roundtrip differs")
        if stored["gain_vs_baseline"] != r["gains"] or stored["disagreement"] != r["margin"]:
            raise ValueError("gain/objective roundtrip differs")
    return dict(file=relative, sha256=sha(container), encoded_sha256=digest)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("run", type=Path)
    p.add_argument("--ffmpeg", type=Path, required=True)
    p.add_argument("--evif", type=Path, required=True)
    p.add_argument("--jobs", type=int, default=1)
    p.add_argument("--output", type=Path, help="fresh directory for a repeated audit")
    a = p.parse_args()
    if not 1 <= a.jobs <= 4:
        raise ValueError("readback jobs must be 1..4")
    root = a.run
    output = root
    if a.output is not None:
        a.output.mkdir()
        output = a.output
    ledger = {r["file"]: r for r in map(json.loads, (root / "ledger.jsonl").read_text().splitlines())}
    files = sorted(root.glob("*/*.evif"))
    unique = set()
    rows = []
    spec = json.loads((root / "SPEC.json").read_text())
    if len(files) != len(spec["views"]) * 3 * (21 + 4 * spec["samples"]):
        raise ValueError("container coverage differs from the fixed budget")
    references = {v["id"]: sha(decode(a.ffmpeg, Path(v["path"]).read_bytes()))
                  for v in spec["views"]}
    # Bound argv length; the native verifier checks every stored pixel digest.
    with (output / "NATIVE_VERIFY.log").open("x") as native_log:
        for offset in range(0, len(files), 64):
            subprocess.run([str(a.evif), "verify", *map(str, files[offset:offset+64])],
                           stdout=native_log, stderr=subprocess.STDOUT, check=True)
    with ThreadPoolExecutor(max_workers=a.jobs) as executor:
        for i, row in enumerate(executor.map(
                lambda path: verify_one(path, root, a.ffmpeg, references, ledger), files)):
            rows.append(row)
            unique.add(row["encoded_sha256"])
            if (i+1) % 128 == 0 or i+1 == len(files):
                progress = dict(pid=os.getpid(), completed=i+1, required=len(files), jobs=a.jobs,
                                stage="independent-readback")
                (output / "ORACLE_PROGRESS.json").write_text(json.dumps(progress, indent=2)+"\n")
                print(json.dumps(progress), flush=True)
    with (output / "ORACLE.json").open("x") as f:
        json.dump(dict(passed=True, files=len(files), unique_bitstreams=len(unique),
                       oracle_binary_sha256=sha(a.ffmpeg.read_bytes()), containers=rows), f, indent=2)
        f.write("\n")


if __name__ == "__main__":
    main()
