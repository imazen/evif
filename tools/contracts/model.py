"""Executable design checks, NOT an EVIF parser or metric runner.

The descriptor subset uses ASCII keys, safe JSON integers, and decimal strings
for physical quantities. That subset can use JCS without float-format ambiguity.
"""
from decimal import Decimal, InvalidOperation
from fractions import Fraction
import hashlib
import json
import math
import re


class ContractError(ValueError):
    pass


def require(condition, message):
    if not condition:
        raise ContractError(message)


def canonical(value):
    def check(v):
        if v is None or type(v) in (str, bool):
            return
        if type(v) is int:
            require(abs(v) <= 2**53 - 1, "integer outside descriptor JSON range")
        elif isinstance(v, list):
            for x in v:
                check(x)
        elif isinstance(v, dict):
            for k, x in v.items():
                require(isinstance(k, str) and k.isascii(), "descriptor keys must be ASCII")
                check(x)
        else:
            raise ContractError("descriptor numbers must be integers or decimal strings")
    check(value)
    # ASCII keys have the same order under Unicode codepoint and UTF-16 sorting.
    # No JSON floating point is admitted; UTF-8 encoding rejects lone surrogates.
    try:
        return json.dumps(value, sort_keys=True, ensure_ascii=False,
                          separators=(",", ":"), allow_nan=False).encode("utf-8")
    except UnicodeError as error:
        raise ContractError("invalid Unicode") from error


def digest(domain, value):
    return hashlib.sha256(b"EVIF/" + domain.encode("ascii") + b"/draft-1\0" + canonical(value)).hexdigest()


def rational(value, *, positive=False):
    require(isinstance(value, list) and len(value) == 2, "expected [numerator, denominator]")
    n, d = value
    require(type(n) is int and type(d) is int and d > 0, "invalid rational")
    require(math.gcd(n, d) == 1, "rational must be reduced")
    require(n > 0 if positive else n >= 0, "invalid rational sign")
    return Fraction(n, d)


def decimal(value):
    require(isinstance(value, str) and re.fullmatch(r"-?(0|[1-9][0-9]*)(\.[0-9]*[1-9])?", value),
            "physical values need canonical decimal strings")
    try:
        number = Decimal(value)
    except InvalidOperation as error:
        raise ContractError("invalid decimal") from error
    require(number.is_finite() and value != "-0", "invalid physical value")
    return number


def timeline(asset):
    require(asset["kind"] in ("still", "sequence"), "unknown media kind")
    require(asset["sample"] in ("u8", "u16", "f16", "f32"), "unsupported sample type")
    require(len(asset["size"]) == 2 and all(type(n) is int and n > 0 for n in asset["size"]), "invalid dimensions")
    require(asset["color"]["authority"] in ("cicp", "icc", "explicit"), "unknown color authority")
    require(asset["alpha"] in ("none", "straight", "premultiplied-linear", "premultiplied-signal"), "ambiguous alpha")
    frames = asset["frames"]
    require(bool(frames), "empty media")
    if asset["kind"] == "still":
        require(len(frames) == 1 and frames[0]["pts"] is None and frames[0]["duration"] is None,
                "still must not invent a video duration")
    end = Fraction(0)
    intervals = []
    for frame in frames:
        require(re.fullmatch(r"[0-9a-f]{64}", frame["samples_sha256"]) is not None, "invalid sample digest")
        if asset["kind"] == "still":
            continue
        start = rational(frame["pts"])
        duration = rational(frame["duration"], positive=True)
        require(start == end, "timeline gap, overlap, or nonzero origin")
        end = start + duration
        intervals.append((start, end))
    return intervals


def pointer(value, path):
    require(path.startswith("/") and "~" not in path, "prototype requires simple JSON pointers")
    try:
        for field in path[1:].split("/"):
            value = value[field]
    except (KeyError, TypeError) as error:
        raise ContractError(f"unresolved condition {path}") from error
    return value


def validate(e):
    canonical(e)
    require(e["schema"] == "evif-evaluation-draft-1", "unsupported critical evaluation schema")
    require(set(e) == {"schema", "inputs", "view", "metric", "pipeline", "output"}, "unknown evaluation fields")
    inputs, metric, pipe = e["inputs"], e["metric"], e["pipeline"]
    require(set(inputs) == {"reference", "candidate"}, "input roles must be explicit")
    candidate = inputs["candidate"]
    times = timeline(candidate)
    if metric["reference_mode"] == "full-reference":
        require(inputs["reference"] is not None, "missing source reference")
        reference_times = timeline(inputs["reference"])
        require(candidate["kind"] == inputs["reference"]["kind"], "media kinds differ")
    else:
        require(metric["reference_mode"] == "no-reference" and inputs["reference"] is None, "invalid reference mode")
        reference_times = times
    require(metric["temporal"] in ("stateless", "stateful", "pooled"), "unknown temporal capability")
    require(metric["implementation"] and metric["model_sha256"] is not None, "metric provenance absent")
    require(metric["one_frame_policy"] in ("still", "sequence", "reject"), "one-frame behavior absent")
    require(pipe["alignment"] in ("exact", "presentation-hold"), "implicit alignment")
    if pipe["alignment"] == "exact":
        require(times == reference_times, "exact alignment needs identical timelines")
    else:
        require(times and reference_times and times[-1][1] == reference_times[-1][1], "alignment cannot silently truncate")
    require(pipe["delivery"]["mode"] in ("still", "native", "cfr-hold"), "implicit frame resampling")
    delivery = pipe["delivery"]
    if delivery["mode"] == "native":
        require(times == reference_times,
                "different native timelines need an explicit delivery map")
    if candidate["kind"] == "still":
        require(delivery["mode"] == "still" and metric["temporal"] == "stateless", "still/video path needs an explicit adapter")
    else:
        require(delivery["mode"] != "still", "sequence collapsed to still")
        if delivery["mode"] == "cfr-hold":
            require(pipe["alignment"] == "presentation-hold", "CFR conversion needs a declared temporal map")
            rate = rational(delivery["rate"], positive=True)
            phase = rational(delivery["phase"])
            require(phase < 1 / rate, "phase outside one sample interval")
            require((times[-1][1] * rate).denominator == 1, "partial final CFR interval requires an explicit trim/pad policy")
        elif metric["temporal"] != "stateless":
            rate = rational(delivery["rate"], positive=True)
            require(all(b-a == 1/rate for a,b in times), "CFR-only adapter cannot consume VFR implicitly")
        if len(times) == 1:
            require(metric["one_frame_policy"] != "reject", "metric refuses a one-frame sequence")
    for side in ("reference", "candidate"):
        steps = pipe[side]
        require(isinstance(steps, list) and bool(steps), "input preparation must be explicit")
        for step in steps:
            require(set(step) == {"op", "implementation", "config"} and step["implementation"], "unversioned preparation step")
    require(pipe["boundary"] in ("reset", "continuous", "replicate", "symmetric", "valid"), "undefined temporal boundary")
    require(isinstance(pipe["context_frames"], dict) and set(pipe["context_frames"]) == {"before", "after"}, "context accounting absent")
    require(all(type(n) is int and n >= 0 for n in pipe["context_frames"].values()), "invalid context frame count")
    view = e["view"]
    require(view["status"] in ("specified", "unknown"), "invalid viewing status")
    binding = metric["view_binding"]
    require(binding["mode"] in ("native", "model-assumed", "not-modeled"), "unknown condition binding")
    require(set(binding) == {"mode", "parameters", "assumptions"}, "condition binding must be explicit")
    if binding["mode"] == "native":
        require(view["status"] == "specified" and binding["parameters"], "native viewing model needs resolved conditions")
    else:
        require(not binding["parameters"], "non-native model cannot claim condition inputs")
    if binding["mode"] == "model-assumed":
        require(binding["assumptions"], "model calibration assumptions absent")
    for parameter, path in binding["parameters"].items():
        require(metric["config"].get(parameter) == pointer(view, path), "requested and effective conditions differ")
    if view["status"] == "specified":
        peak, black = decimal(view["display"]["peak_nits"]), decimal(view["display"]["black_nits"])
        require(peak > black >= 0, "invalid display luminance range")
        require(decimal(view["geometry"]["ppd"]) > 0, "invalid angular geometry")
        require(decimal(view["ambient"]["illuminance_lux"]) >= 0, "invalid ambient illumination")
    output = e["output"]
    require(output["scope"] in ("frame", "window", "clip"), "unknown output scope")
    require(output["kind"] in ("quality", "difference", "feature", "probability", "decibels"), "unknown output meaning")
    require(output["direction"] in ("higher", "lower", "none"), "unknown quality direction")
    if output["kind"] == "feature":
        require(output["direction"] == "none", "feature cannot silently become a quality target")
    pooling = output["pooling"]
    require(pooling["owner"] in ("none", "metric", "wrapper"), "pooling provenance absent")
    if times and output["scope"] == "clip" and metric["temporal"] == "stateless":
        require(pooling["owner"] == "wrapper", "stateless metric has no native clip quality")
        require(pooling["method"] in ("frame-mean", "duration-mean", "minkowski", "percentile"), "unknown wrapper pooling")
    if metric["temporal"] == "pooled":
        require(output["scope"] == "clip" and pooling["owner"] == "metric", "pooled scorer does not promise per-frame scores")
    return e


def evaluation_id(e):
    return digest("evaluation", validate(e))


def asset_id(asset):
    canonical(asset)
    timeline(asset)
    return digest("media", asset)


def frame_at(asset, time):
    for index, (start, end) in enumerate(timeline(asset)):
        if start <= time < end:
            return index
    raise ContractError("sample outside presentation span")


def delivery_pairs(e):
    """Small-fixture iterator only: production must stream indexes and rasters."""
    validate(e)
    d = e["pipeline"]["delivery"]
    r, c = e["inputs"]["reference"], e["inputs"]["candidate"]
    if d["mode"] != "cfr-hold":
        return [(i if r is not None else None, i) for i in range(len(c["frames"]))]
    rate, phase = rational(d["rate"], positive=True), rational(d["phase"])
    count = int(timeline(c)[-1][1] * rate)
    require(count <= 4096, "prototype sample limit; production requires a streaming iterator")
    return [(frame_at(r, phase + Fraction(i, 1)/rate) if r else None,
             frame_at(c, phase + Fraction(i, 1)/rate)) for i in range(count)]


def duration_mean(values, durations):
    require(len(values) == len(durations) and bool(values), "pool coverage mismatch")
    weights = [rational(d, positive=True) for d in durations]
    require(all(math.isfinite(v) for v in values), "nonfinite arithmetic needs a declared reduction")
    return sum(Fraction(v) * w for v, w in zip(values, weights)) / sum(weights)


def validate_result(e, result):
    require(result["evaluation_id"] == evaluation_id(e), "result belongs to another evaluation")
    require(result["status"] in ("complete", "partial", "failed", "unsupported"), "unknown result status")
    require(set(result) == {"evaluation_id", "status", "requested", "completed", "value"}, "unknown result fields")
    requested, completed = result["requested"], result["completed"]
    require(isinstance(requested, list) and isinstance(completed, list), "coverage needs frame identities")
    require(all(type(i) is int for i in requested + completed), "invalid frame identity")
    require(requested == list(range(len(delivery_pairs(e)))), "requested coverage differs from delivery plan")
    require(len(completed) == len(set(completed)) and set(completed) <= set(requested), "duplicate or unknown completed frame")
    if result["status"] != "complete":
        require(result["value"] is None, "incomplete result cannot masquerade as a complete score")
        return
    require(bool(requested) and set(requested) == set(completed), "incomplete coverage")
    value = result["value"]
    require(set(value) in ({"finite"}, {"special"}), "invalid numeric result")
    if "finite" in value:
        decimal(value["finite"])
    else:
        require(value["special"] == "+inf" and e["output"]["kind"] == "decibels", "undefined nonfinite result")
