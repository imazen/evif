"""Resolved SDR/HDR rendition design checks, not an EVIF reader or decoder."""
import re

from model import canonical, decimal, digest, rational, require, timeline


def sha256(value):
    require(isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value),
            "invalid SHA-256")


def validate_rendition(media):
    canonical(media)
    require(isinstance(media, dict) and set(media) == {
        "schema", "kind", "size", "layout", "sample", "alpha", "pixel_aspect",
        "scan", "orientation", "composition", "signal", "plays", "frames",
    }, "unknown or missing resolved-rendition fields")
    require(media["schema"] == "evif-resolved-rendition-draft-1", "unknown rendition schema")
    require(isinstance(media["size"], list) and len(media["size"]) == 2,
            "dimensions must be [width, height]")
    require(media["layout"] in ("gray", "gray-alpha", "rgb", "rgba"), "unsupported layout")
    require(media["alpha"] == ("straight" if media["layout"] in ("gray-alpha", "rgba") else "none"),
            "layout/alpha mismatch")
    rational(media["pixel_aspect"], positive=True)
    require(media["scan"] == "progressive" and media["orientation"] == "identity",
            "resolve scan/orientation before storage")
    require(media["composition"] == "complete-frames", "unresolved frame composition")
    signal = media["signal"]
    require(isinstance(signal, dict) and set(signal) == {
        "primaries", "transfer", "domain", "units", "nits_per_unit",
    }, "incomplete signal interpretation")
    require(signal["primaries"] in ("bt709", "display-p3-d65", "bt2020"), "unsupported primaries")
    domains = {
        "srgb": "encoded-display-relative",
        "bt709": "encoded-scene-relative",
        "pq": "encoded-display-absolute",
        "hlg": "encoded-scene-relative",
    }
    transfer, domain = signal["transfer"], signal["domain"]
    require(isinstance(transfer, str) and isinstance(domain, str), "invalid transfer/domain")
    if transfer == "linear":
        if domain == "linear-display-absolute":
            require(media["sample"] in ("f16", "f32"), "absolute linear requires float samples")
            require(signal["units"] == "nits" and decimal(signal["nits_per_unit"]) > 0,
                    "absolute linear requires a positive nit scale")
        else:
            require(domain in ("linear-scene-relative", "linear-display-relative"),
                    "ambiguous linear domain")
            require(signal["units"] == "relative" and signal["nits_per_unit"] is None,
                    "relative linear cannot imply a nit scale")
    else:
        require(transfer in domains and domain == domains[transfer], "transfer/domain mismatch")
        require(signal["units"] == "normalized" and signal["nits_per_unit"] is None,
                "encoded signal cannot add a second luminance scale")
    if media["kind"] == "still":
        require(media["plays"] is None, "still has no loop count")
    else:
        require(type(media["plays"]) is int and media["plays"] >= 0, "invalid total play count")
    require(isinstance(media["frames"], list), "frames must be ordered")
    for frame in media["frames"]:
        require(isinstance(frame, dict) and set(frame) == {"pts", "duration", "samples_sha256"},
                "unknown or missing frame fields")
        sha256(frame["samples_sha256"])
    # Reuse the evaluation prototype's exact interval checks; color here is fully
    # checked above, rather than delegated to that prototype's looser vocabulary.
    return timeline({**media, "color": {"authority": "explicit"}})


def rendition_id(media):
    validate_rendition(media)
    return digest("resolved-rendition", media)


def validate_storage(media, storage):
    """Check an index plan. Payload decoding and byte/hash matching are separate."""
    validate_rendition(media)
    canonical(storage)
    require(isinstance(storage, dict) and set(storage) == {"segments"}, "invalid storage plan")
    segments = storage["segments"]
    require(isinstance(segments, list) and bool(segments), "empty segment index")
    next_frame = 0
    for segment in segments:
        require(isinstance(segment, dict) and set(segment) == {
            "carrier", "first_frame", "frame_count", "location", "payload_sha256",
        }, "unknown or missing segment fields")
        require(type(segment["first_frame"]) is int and segment["first_frame"] == next_frame,
                "segment gap, overlap or reordering")
        count = segment["frame_count"]
        require(type(count) is int and count > 0, "invalid segment frame count")
        carrier = segment["carrier"]
        require(carrier in ("png", "apng", "imi"), "unsupported carrier")
        require(carrier != "png" or count == 1, "PNG is a single frame")
        require(carrier == "imi" or media["sample"] in ("u8", "u16"), "PNG cannot preserve float samples")
        next_frame += count
        require(next_frame <= len(media["frames"]), "segment exceeds media")
        if carrier == "apng" and media["kind"] == "sequence":
            for frame in media["frames"][segment["first_frame"]:next_frame]:
                duration = rational(frame["duration"], positive=True)
                require(duration.numerator <= 65535 and duration.denominator <= 65535,
                        "duration cannot be represented exactly by APNG")
        location = segment["location"]
        require(isinstance(location, dict) and len(location) == 1 and
                set(location) <= {"embedded", "external"}, "invalid payload location")
        require(all(isinstance(v, str) and bool(v) for v in location.values()), "empty payload location")
        if segment["payload_sha256"] is not None:
            sha256(segment["payload_sha256"])
        require("external" not in location or segment["payload_sha256"] is not None,
                "external payload needs an exact content hash")
    require(next_frame == len(media["frames"]), "incomplete segment coverage")
