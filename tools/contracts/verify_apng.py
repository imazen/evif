"""Independent read-back of the synthetic APNG carrier tests; no encoding."""
import argparse
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import zlib


def verify(root):
    results = []
    for transfer in (16, 18):
        path = root / f"transfer-{transfer}.png"
        data = path.read_bytes()
        assert data[:8] == b"\x89PNG\r\n\x1a\n"
        pos = 8
        timing, cicp = [], None
        while pos < len(data):
            length = struct.unpack_from(">I", data, pos)[0]
            tag = data[pos + 4:pos + 8]
            body = data[pos + 8:pos + 8 + length]
            crc = struct.unpack_from(">I", data, pos + 8 + length)[0]
            assert zlib.crc32(tag + body) == crc
            if tag == b"fcTL":
                timing.append(list(struct.unpack_from(">HH", body, 20)))
            if tag == b"cICP":
                cicp = list(body)
            pos += 12 + length
        assert pos == len(data)
        assert timing == [[1, 24], [1001, 30000], [7, 100]]
        assert cicp == [9, transfer, 0, 1]
        cmd = ["ffmpeg", "-v", "error", "-threads", "1", "-ignore_loop", "1",
               "-i", str(path), "-frames:v", "3", "-fps_mode", "passthrough",
               "-f", "rawvideo", "-pix_fmt", "rgba64le", "pipe:1"]
        decoded = subprocess.run(cmd, check=True, capture_output=True).stdout
        assert decoded == (root / f"transfer-{transfer}.rgba64le").read_bytes()
        results.append(dict(transfer=transfer, frames=3, all_samples_exact=True,
                            timing=timing, cicp=cicp, command=cmd,
                            sha256=hashlib.sha256(data).hexdigest()))
    return {"ffmpeg": subprocess.check_output(["ffmpeg", "-version"], text=True).splitlines()[0],
            "cases": results}


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("artifacts", type=Path)
    args = ap.parse_args()
    print(json.dumps(verify(args.artifacts), indent=2))
