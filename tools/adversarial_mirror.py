#!/usr/bin/env python3
"""Verify a sealed archive's sampled files on a local or signed-URL mirror.

Signed URLs are read from a private file and are never included in receipts.
Local mirrors receive a full SHA256 check; remote mirrors receive size,
SHA256-metadata and exact byte-range checks for the sealed random sample.
"""
import argparse
import hashlib
import json
from pathlib import Path
import tarfile
from urllib.parse import urlsplit, urlunsplit
from urllib.request import Request, urlopen


def digest(stream, length=None):
    h = hashlib.sha256()
    remaining = length
    while remaining is None or remaining > 0:
        block = stream.read(1024 * 1024 if remaining is None else min(1024 * 1024, remaining))
        if not block:
            if remaining:
                raise ValueError("mirror range truncated")
            break
        h.update(block)
        if remaining is not None:
            remaining -= len(block)
    return h.hexdigest()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("archive", type=Path)
    p.add_argument("receipt", type=Path)
    p.add_argument("output", type=Path)
    dest = p.add_mutually_exclusive_group(required=True)
    dest.add_argument("--local-mirror", type=Path)
    dest.add_argument("--signed-url-file", type=Path)
    a = p.parse_args()
    if a.output.exists():
        raise ValueError("fresh mirror receipt required")
    receipt = json.loads(a.receipt.read_text())
    if a.archive.stat().st_size != receipt["bytes"] or len(receipt["verified_random_files"]) != 3:
        raise ValueError("source archive identity or sample count differs")
    url = None
    full_sha = None
    if a.local_mirror is not None:
        if a.local_mirror.stat().st_size != receipt["bytes"]:
            raise ValueError("local mirror length differs")
        with a.local_mirror.open("rb") as f:
            full_sha = digest(f)
        if full_sha != receipt["sha256"]:
            raise ValueError("local mirror SHA256 differs")
        mirror = str(a.local_mirror)
    else:
        url = a.signed_url_file.read_text().strip()
        parsed = urlsplit(url)
        if parsed.scheme != "https":
            raise ValueError("signed archive mirror requires HTTPS")
        mirror = urlunsplit((parsed.scheme, parsed.netloc, parsed.path, "", ""))
    checks = []
    with tarfile.open(a.archive, "r:") as archive:
        roots = {m.name.split("/")[0] for m in archive.getmembers()}
        if len(roots) != 1:
            raise ValueError("unexpected archive roots")
        root = roots.pop()
        for sample in receipt["verified_random_files"]:
            member = archive.getmember(root + "/" + sample["path"])
            if not member.isfile():
                raise ValueError("sample must be an ordinary file")
            # An empty file is authenticated through its archive header too.
            offset = member.offset_data if member.size else member.offset
            count = member.size if member.size else 512
            expected = sample["sha256"]
            if not member.size:
                with a.archive.open("rb") as f:
                    f.seek(offset)
                    expected = digest(f, count)
                if sample["sha256"] != hashlib.sha256(b"").hexdigest():
                    raise ValueError("empty sample SHA256 differs")
            if url is None:
                with a.local_mirror.open("rb") as f:
                    f.seek(offset)
                    actual = digest(f, count)
            else:
                last = offset + count - 1
                request = Request(url, headers={"Range": f"bytes={offset}-{last}"})
                with urlopen(request, timeout=30) as response:
                    if (response.status != 206
                            or response.headers.get("Content-Range") != f"bytes {offset}-{last}/{receipt['bytes']}"
                            or int(response.headers["Content-Length"]) != count
                            or response.headers.get("x-amz-meta-sha256") != receipt["sha256"]):
                        raise ValueError("remote range/size/SHA256 metadata differs")
                    actual = digest(response, count)
            if actual != expected:
                raise ValueError("mirror sampled-file range differs")
            checks.append(dict(path=sample["path"], offset=offset, range_bytes=count,
                               file_bytes=member.size, range_sha256=actual, file_sha256=sample["sha256"]))
    result = dict(build_commit=receipt["build_commit"], verified=True, mirror=mirror,
                  source_archive_sha256=receipt["sha256"], bytes=receipt["bytes"],
                  full_local_mirror_sha256=full_sha, sampled_ranges=checks)
    with a.output.open("x") as f:
        json.dump(result, f, indent=2)
        f.write("\n")
    print(json.dumps(dict(verified=True, bytes=result["bytes"], sampled_files=len(checks),
                          full_local_sha256_checked=full_sha is not None)), flush=True)


if __name__ == "__main__":
    main()
