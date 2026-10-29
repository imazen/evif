#!/usr/bin/env python3
"""Seal a validated pilot, retain reference/model/tool bytes, and verify a tar archive."""
import argparse
import hashlib
import json
from pathlib import Path
import random
import shutil
import tarfile

from adversarial_models import PINS
from adversarial_verify import chunks


def sha(path):
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def read(path):
    return json.loads(path.read_text())


def retain(source, dest, expected=None):
    digest = sha(source)
    if expected is not None and digest != expected:
        raise ValueError(f"input hash differs: {source}")
    dest.parent.mkdir(parents=True, exist_ok=True)
    if dest.exists():
        if sha(dest) != digest:
            raise ValueError(f"existing retained file differs: {dest}")
    else:
        with source.open("rb") as src, dest.open("xb") as dst:
            shutil.copyfileobj(src, dst, 1024 * 1024)
    return digest


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("run", type=Path)
    p.add_argument("archive", type=Path)
    p.add_argument("--models", type=Path, required=True)
    p.add_argument("--binary", type=Path, required=True)
    p.add_argument("--binary-sha256", required=True)
    p.add_argument("--binary-commit")
    p.add_argument("--tool-commit")
    a = p.parse_args()
    root = a.run
    if a.archive.exists() or (root / "_MANIFEST.json").exists():
        raise ValueError("fresh archive and unsealed run required; preserve earlier receipts")
    if a.archive.resolve().is_relative_to(root.resolve()):
        raise ValueError("archive must be outside its input tree")
    spec = read(root / "SPEC.json")
    complete = read(root / "COMPLETE.json")
    controls = read(root / "CONTROL.json")
    oracle = read(root / "ORACLE.json")
    summary = read(root / "SUMMARY.json")
    partitions = read(root / "PARTITIONS.json")
    n = len(spec["views"])
    expected = n * 3 * (21 + spec["samples"] * 4)
    if (complete["views"] != n or complete["completed_arms"] != n * 12
            or not oracle["passed"] or oracle["files"] != expected
            or len(oracle["containers"]) != expected
            or summary["search_encodes"] != n * 12 * spec["samples"]
            or summary["baseline_encodes"] != n * 3 * 21
            or sha(root / "ledger.jsonl") != summary["ledger_sha256"]
            or len(controls) != 4 or not all(c["pass"] for c in controls)):
        raise ValueError("pilot completion, budget, control or readback gate failed")

    references = []
    for v in spec["views"]:
        dest = root / "references" / (v["id"] + ".png")
        digest = retain(Path(v["path"]), dest, v["sha256"])
        references.append(dict(view=v["id"], path=str(dest.relative_to(root)), sha256=digest))
    models = []
    for profile, expected_sha in PINS.items():
        name = f"rev7_{profile}_h128_2026-10-27.bin"
        dest = root / "models" / name
        digest = retain(a.models / name, dest, expected_sha)
        models.append(dict(profile=profile, path=str(dest.relative_to(root)), sha256=digest))
    binary = root / "binaries" / a.binary.name
    binary_sha = retain(a.binary, binary, a.binary_sha256)
    binary.chmod(0o755)
    for name in ["adversarial_archive.py", "adversarial_models.py", "adversarial_verify.py"]:
        retain(Path(__file__).parent / name, root / "tools" / name)

    files = []
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            raise ValueError(f"symlink in sealed input: {path}")
        if path.is_file():
            files.append(dict(path=str(path.relative_to(root)), bytes=path.stat().st_size, sha256=sha(path)))
            if len(files) % 10000 == 0:
                print(json.dumps(dict(stage="seal-hashes", files=len(files))), flush=True)
    by_path = {f["path"]: f for f in files}
    if sum(f["path"].endswith(".evif") for f in files) != expected:
        raise ValueError("sealed EVIF coverage differs")
    for c in oracle["containers"]:
        if by_path[c["file"]]["sha256"] != c["sha256"]:
            raise ValueError("container changed after readback")
        if by_path[c["artifact"]]["sha256"] != c["encoded_sha256"]:
            raise ValueError("native CAS changed after readback")
        scores = json.loads(chunks((root / c["file"]).read_bytes())[b"scor"])["scores"]
        for s in scores:
            if s["metric"] == "butteraugli":
                m = s["diffmap"]
                if by_path[m["path"]]["sha256"] != m["sha256"]:
                    raise ValueError("native map changed after readback")
    for role, name in [("training-leg", "TRAINING-LEG.jsonl"), ("gauntlet", "GAUNTLET.jsonl")]:
        for line in (root / name).read_text().splitlines():
            r = json.loads(line)
            if partitions[r["source"]]["partition"] != role or r["eligible_for_training"] != (role == "training-leg"):
                raise ValueError("find/source reservation mismatch")
            if by_path[r["artifact"]]["sha256"] != r["encoded_sha256"]:
                raise ValueError("find index native CAS mismatch")
    manifest = dict(schema=1, build_commit=spec["build_commit"], verified=True,
                    archive_tool_commit=a.tool_commit, archive_tool_sha256=sha(Path(__file__)),
                    binary=dict(path=str(binary.relative_to(root)), sha256=binary_sha, source_commit=a.binary_commit),
                    models=models, references=references, views=n, containers=expected, files=files)
    with (root / "_MANIFEST.json").open("x") as f:
        json.dump(manifest, f, indent=2, allow_nan=False)
        f.write("\n")
    manifest_sha = sha(root / "_MANIFEST.json")

    def portable(info):
        info.uid = info.gid = 0
        info.uname = info.gname = ""
        return info

    with tarfile.open(a.archive, "x") as archive:
        archive.add(root, arcname=root.name, filter=portable)
    checks = []
    selected = random.Random(int(manifest_sha[:16], 16)).sample(files, 3)
    with tarfile.open(a.archive, "r:") as archive:
        for item in selected:
            h = hashlib.sha256()
            with archive.extractfile(root.name + "/" + item["path"]) as f:
                for block in iter(lambda: f.read(1024 * 1024), b""):
                    h.update(block)
            if h.hexdigest() != item["sha256"]:
                raise ValueError("archive random-file readback differs")
            checks.append(dict(path=item["path"], sha256=h.hexdigest()))
    receipt = dict(build_commit=spec["build_commit"], archive=str(a.archive),
                   bytes=a.archive.stat().st_size, sha256=sha(a.archive), manifest_sha256=manifest_sha,
                   verified_random_files=checks, files=len(files) + 1)
    with a.archive.with_suffix(a.archive.suffix + ".json").open("x") as f:
        json.dump(receipt, f, indent=2)
        f.write("\n")
    print(json.dumps(receipt), flush=True)


if __name__ == "__main__":
    main()
