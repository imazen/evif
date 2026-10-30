#!/usr/bin/env python3
"""Independent-decoder cross-check of stored EVIF samples (third-party oracle, test use only).

usage: oracle_check.py DIR  > receipt.json
For every .evif under DIR: take the original encoded bytes (BITS chunk) and the stored decoded pixels
(PNGF chunk); decode the bytes with an independent implementation (libjpeg-turbo `djpeg` for JPEG, with the
embedded ICC profile applied through lcms for XYB streams; libavif `avifdec` for AVIF) and compare with the
pixels the EVIF stores. A disagreement is a finding, not a vote: it is recorded with both numbers.
"""
import io, json, struct, subprocess, sys, tempfile
from pathlib import Path

import numpy as np
from PIL import Image, ImageCms

root = Path(sys.argv[1])
sample = {}
if (root / "oracle-sample.json").exists():
    for s in json.loads((root / "oracle-sample.json").read_text()):
        sample[s["file"]] = s


def chunks(d):
    off, out = 12, {}
    while off < len(d):
        tag = d[off:off + 4].decode()
        n = struct.unpack_from("<I", d, off + 4)[0]
        off += 8
        out[tag] = d[off:off + n]
        off += n
    return out


def srgb():
    return ImageCms.createProfile("sRGB")


results, problems = [], []
tmp = Path(tempfile.mkdtemp(dir=Path.home() / "tmp"))
for path in sorted(root.rglob("*.evif")):
    rel = str(path.relative_to(root))
    # strip the shard directory the copy kept
    key = "/".join(rel.split("/")[1:])
    meta = sample.get(key, {})
    c = chunks(path.read_bytes())
    bits = c["BITS"]
    codec, data = bits[:4], bits[4:]
    ours = np.asarray(Image.open(io.BytesIO(c["PNGF"])).convert("RGB")).astype(np.int16)
    row = dict(file=rel, family=meta.get("family"), kind=meta.get("kind"), rung=meta.get("rung"), ramp=meta.get("ramp"),
               codec=codec.decode(), bytes=len(data), shape=list(ours.shape))
    try:
        if codec == b"jpeg":
            jpg = tmp / "x.jpg"
            jpg.write_bytes(data)
            icc = Image.open(jpg).info.get("icc_profile")
            row["icc_bytes"] = len(icc) if icc else 0
            ppm = subprocess.run(["djpeg", "-ppm", str(jpg)], capture_output=True, check=True).stdout
            img = Image.open(io.BytesIO(ppm)).convert("RGB")
            if icc:
                prof = ImageCms.ImageCmsProfile(io.BytesIO(icc))
                t = ImageCms.buildTransform(prof, srgb(), "RGB", "RGB", renderingIntent=0)
                img = ImageCms.applyTransform(img, t)
            theirs = np.asarray(img).astype(np.int16)
            row["decoder"] = "libjpeg-turbo djpeg" + (" + lcms ICC" if icc else "")
        elif codec == b"avif":
            avif = tmp / "x.avif"
            avif.write_bytes(data)
            png = tmp / "x.png"
            subprocess.run(["avifdec", str(avif), str(png)], capture_output=True, check=True)
            theirs = np.asarray(Image.open(png).convert("RGB")).astype(np.int16)
            row["decoder"] = "libavif avifdec"
        else:
            raise ValueError(f"unknown codec {codec!r}")
        if theirs.shape != ours.shape:
            row["error"] = f"shape differs {theirs.shape} vs {ours.shape}"
        else:
            d = np.abs(theirs - ours)
            mse = float((d.astype(np.float64) ** 2).mean())
            row.update(max_abs=int(d.max()), mean_abs=float(d.mean()), p999=float(np.percentile(d, 99.9)),
                       psnr=(10 * np.log10(255 * 255 / mse) if mse > 0 else 99.0))
    except Exception as e:  # recorded, not hidden
        row["error"] = repr(e)[:300]
    results.append(row)
    if "error" in row:
        problems.append(row)

json.dump(dict(n=len(results), errors=len(problems), rows=results), sys.stdout, indent=1)
