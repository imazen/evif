"""Independent small-fixture reader for SPEC.md; optional FFmpeg APNG read-back.

This is test tooling, not a production parser or an encoder. No Rust code is used
for framing, roots, SHA-256, or the optional decoded-sample check. Decoded-sample
verification requires the system xxHash shared library and FFmpeg.
"""
import argparse
import ctypes
import ctypes.util
from fractions import Fraction
import hashlib
import json
from pathlib import Path
import struct
import subprocess
import zlib


def pairs(items):
    result = {}
    for key, value in items:
        if key in result:
            raise ValueError("duplicate JSON key")
        result[key] = value
    return result


def exact(source, size):
    data = source.read(size)
    if len(data) != size:
        raise ValueError("truncated record")
    return data


def u64(value):
    assert isinstance(value, str) and value.isascii() and value.isdecimal()
    n = int(value)
    assert 0 <= n < 2**64 and str(n) == value
    return n


def ratio(value):
    n, d = u64(value["num"]), u64(value["den"])
    assert n > 0 and d > 0 and Fraction(n, d).numerator == n
    return struct.pack("<QQ", n, d)


def code(value, names):
    return names.split().index(value) + 1


def initial_hash(a):
    assert a["profile"] == "resolved-rgb/1"
    h = hashlib.sha256(b"EVIF/media/2\0")
    h.update(bytes([1, code(a["kind"], "still sequence")]))
    h.update(struct.pack("<II", a["width"], a["height"]))
    h.update(bytes([
        code(a["channels"], "gray gray-alpha rgb rgba"),
        code(a["samples"], "u8 u16 f16 f32"),
        code(a["primaries"], "bt709 display-p3-d65 bt2020"),
        code(a["transfer"], "srgb bt709 pq hlg linear"),
        code(a["domain"], "display-relative scene-relative display-absolute"),
    ]))
    h.update(bytes([a.get("nits_per_unit") is not None]))
    if a.get("nits_per_unit") is not None:
        h.update(ratio(a["nits_per_unit"]))
    h.update(ratio(a["pixel_aspect"]))
    h.update(struct.pack("<Q", u64(a["frame_count"])))
    h.update(bytes([a.get("plays") is not None]))
    if a.get("plays") is not None:
        h.update(struct.pack("<Q", u64(a["plays"])))
    return h


def xxh3(data):
    library = ctypes.util.find_library("xxhash")
    if library is None:
        raise RuntimeError("decoded-sample verification requires the xxHash shared library")
    function = ctypes.CDLL(library).XXH3_64bits
    function.argtypes = (ctypes.c_void_p, ctypes.c_size_t)
    function.restype = ctypes.c_uint64
    return f"{function(data, len(data)):016x}"


def check_apng(a, segment, body, path):
    # The generated fixture profile is RGBA16, full-canvas PQ/HLG; reject another
    # profile instead of silently omitting sample verification.
    assert a["channels"] == "rgba" and a["samples"] == "u16"
    assert segment["carrier"] == "apng"
    assert body[:8] == b"\x89PNG\r\n\x1a\n"
    pos, timing, cicp = 8, [], None
    while pos < len(body):
        size = struct.unpack_from(">I", body, pos)[0]
        tag = body[pos+4:pos+8]
        data = body[pos+8:pos+8+size]
        assert len(data) == size
        crc = struct.unpack_from(">I", body, pos+8+size)[0]
        assert zlib.crc32(tag + data) == crc
        if tag == b"IHDR":
            w, h, bits, layout = struct.unpack_from(">IIBB", data)
            assert (w, h, bits, layout) == (a["width"], a["height"], 16, 6)
        elif tag == b"cICP":
            cicp = list(data)
        elif tag == b"fcTL":
            n, d = struct.unpack_from(">HH", data, 20)
            timing.append(Fraction(n, d or 100))
        pos += size + 12
    assert pos == len(body)
    assert cicp == [9, {"pq":16, "hlg":18}[a["transfer"]], 0, 1]
    assert timing == [Fraction(u64(f["duration"]["num"]), u64(f["duration"]["den"])) for f in segment["frames"]]
    # FFmpeg's APNG demuxer requires a seekable input. Retain the exact extracted
    # bytes beside the fixture instead of using an ephemeral pipe or /tmp file.
    extracted = path.parent / "wire-oracle" / f"{path.stem}-asset-{a['id']}-frame-{segment['first_frame']}.png"
    extracted.parent.mkdir(exist_ok=True)
    if extracted.exists():
        assert extracted.read_bytes() == body
    else:
        with extracted.open("xb") as output:
            output.write(body)
    cmd = ["ffmpeg", "-v", "error", "-threads", "1", "-ignore_loop", "1",
           "-f", "apng", "-i", str(extracted), "-frames:v", str(len(timing)), "-fps_mode", "passthrough",
           "-f", "rawvideo", "-pix_fmt", "rgba64le", "pipe:1"]
    result = subprocess.run(cmd, capture_output=True)
    if result.returncode:
        raise RuntimeError(result.stderr.decode("utf-8", errors="replace"))
    decoded = result.stdout
    size = a["width"] * a["height"] * 8
    assert len(decoded) == size * len(timing)
    prefix = b"EVIF/frame/1\0" + struct.pack("<IIBB", a["width"], a["height"], 4, 2)
    for i, frame in enumerate(segment["frames"]):
        canonical = prefix + decoded[i*size:(i+1)*size]
        assert xxh3(canonical) == frame["samples"]["xxh3-64"]
        if frame["samples"].get("sha256") is not None:
            assert hashlib.sha256(canonical).hexdigest() == frame["samples"]["sha256"]


def inspect(path, decode_apng=False):
    assets, hashes, counts, scores = {}, {}, {}, []
    pending, ended, records, decoded_segments = None, False, 0, 0
    with path.open("rb") as source:
        assert exact(source, 16) == b"\x8bEVIF\r\n\x1a\x01\0\0\0\0\0\0\0"
        while prefix := source.read(12):
            assert len(prefix) == 12
            tag, size = struct.unpack("<4sQ", prefix)
            assert size <= 16*1024*1024, "oracle is for small fixtures"
            body = exact(source, size)
            records += 1
            if pending is not None:
                assert tag == (b"IMIJ" if pending["carrier"] == "imi" else b"PNGF")
                expected = pending["storage"].get("sha256")
                if expected is not None:
                    assert hashlib.sha256(body).hexdigest() == expected
                if decode_apng:
                    check_apng(assets[pending["asset"]], pending, body, path)
                    decoded_segments += 1
                pending = None
                continue
            if tag in (b"ASST", b"SEGM", b"MEND", b"scor"):
                value = json.loads(body.decode("utf-8"), object_pairs_hook=pairs)
            if tag == b"ASST":
                assert not ended and value["id"] not in assets
                assets[value["id"]] = value
                hashes[value["id"]] = initial_hash(value)
                counts[value["id"]] = 0
            elif tag == b"SEGM":
                assert not ended
                key = value["asset"]
                assert u64(value["first_frame"]) == counts[key]
                for frame in value["frames"]:
                    duration = frame.get("duration")
                    hashes[key].update(bytes([duration is not None]))
                    if duration is not None:
                        hashes[key].update(ratio(duration))
                    sample_hash = frame["samples"]["xxh3-64"]
                    assert len(sample_hash) == 16 and all(c in "0123456789abcdef" for c in sample_hash)
                    hashes[key].update(struct.pack("<Q", int(sample_hash, 16)))
                    counts[key] += 1
                if value["storage"]["kind"] == "embedded":
                    pending = value
                else:
                    assert not decode_apng, "external samples unavailable to oracle"
            elif tag == b"MEND":
                assert not ended
                assert all(counts[k] == u64(a["frame_count"]) for k, a in assets.items())
                assert value == [{"asset":k,"sha256":hashes[k].hexdigest()} for k in sorted(assets)]
                ended = True
            elif tag == b"scor":
                assert ended and value["candidate"] in [h.hexdigest() for h in hashes.values()]
                assert value["id"] not in scores
                assert value.get("supersedes") is None or value["supersedes"] in scores
                scores.append(value["id"])
            elif tag == b"BITS":
                assert not ended and len(body) > 8 and struct.unpack_from("<I", body)[0] in assets
            else:
                assert tag[:1].islower(), "unknown critical record"
        assert ended and pending is None
    return {"file":str(path), "assets":len(assets), "records":records,
            "media":{str(k):h.hexdigest() for k,h in hashes.items()},
            "scores":scores, "decoded_apng_segments":decoded_segments}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("files", nargs="+", type=Path)
    parser.add_argument("--decode-apng", action="store_true")
    args = parser.parse_args()
    print(json.dumps([inspect(p, args.decode_apng) for p in args.files], indent=2))
