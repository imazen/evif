"""Counterexamples for resolved rendition descriptors; no decoder claims."""
from copy import deepcopy
import unittest

from model import ContractError
from rendition import rendition_id, validate_rendition, validate_storage


def media(transfer="pq", sample="u16"):
    domains = {"srgb": "encoded-display-relative", "bt709": "encoded-scene-relative",
               "pq": "encoded-display-absolute", "hlg": "encoded-scene-relative",
               "linear": "linear-display-absolute"}
    return {
        "schema": "evif-resolved-rendition-draft-1",
        "kind": "sequence", "size": [13, 9], "layout": "rgba", "sample": sample,
        "alpha": "straight", "pixel_aspect": [1, 1], "scan": "progressive",
        "orientation": "identity", "composition": "complete-frames", "plays": 1,
        "signal": {"primaries": "bt709" if transfer in ("srgb", "bt709") else "bt2020",
                   "transfer": transfer, "domain": domains[transfer],
                   "units": "nits" if transfer == "linear" else "normalized",
                   "nits_per_unit": "1" if transfer == "linear" else None},
        # Illustrative hashes, not claims of decoded samples.
        "frames": [{"pts": [0, 1], "duration": [1, 24], "samples_sha256": "a" * 64},
                   {"pts": [1, 24], "duration": [1, 12], "samples_sha256": "b" * 64}],
    }


def segment(carrier="apng", first=0, count=2):
    return {"carrier": carrier, "first_frame": first, "frame_count": count,
            "location": {"embedded": f"payload-{first}"}, "payload_sha256": None}


class RenditionTests(unittest.TestCase):
    def test_sdr_pq_hlg_and_absolute_float_without_gain_maps(self):
        for transfer, sample, carrier in [
            ("srgb", "u8", "apng"), ("bt709", "u16", "imi"),
            ("pq", "u16", "apng"), ("hlg", "u16", "apng"),
            ("linear", "f16", "imi"), ("linear", "f32", "imi"),
        ]:
            with self.subTest(transfer=transfer, sample=sample):
                candidate = media(transfer, sample)
                validate_rendition(candidate)
                validate_storage(candidate, {"segments": [segment(carrier)]})

    def test_sample_interpretation_and_timing_are_identity(self):
        candidate = media()
        changes = [
            lambda m: m.update(pixel_aspect=[4, 3]),
            lambda m: m.update(plays=0),
            lambda m: m["signal"].update(primaries="display-p3-d65"),
            lambda m: m["signal"].update(transfer="hlg", domain="encoded-scene-relative"),
            lambda m: m["frames"][1].update(duration=[1, 24]),
            lambda m: m["frames"][0].update(samples_sha256="c" * 64),
        ]
        for change in changes:
            changed = deepcopy(candidate)
            change(changed)
            self.assertNotEqual(rendition_id(candidate), rendition_id(changed))

    def test_relative_and_absolute_linear_are_distinct(self):
        candidate = media("linear", "f32")
        absolute = rendition_id(candidate)
        for domain in ("linear-scene-relative", "linear-display-relative"):
            candidate["signal"].update(domain=domain, units="relative", nits_per_unit=None)
            self.assertNotEqual(absolute, rendition_id(candidate))
        candidate["signal"]["nits_per_unit"] = "100"
        with self.assertRaisesRegex(ContractError, "relative linear"):
            validate_rendition(candidate)

    def test_no_implicit_linear_scale_or_hlg_display_transform(self):
        for scale in (None, "0", "-1", "NaN"):
            candidate = media("linear", "f32")
            candidate["signal"]["nits_per_unit"] = scale
            with self.assertRaises(ContractError):
                validate_rendition(candidate)
        for transfer in ("pq", "hlg"):
            candidate = media(transfer)
            candidate["signal"]["nits_per_unit"] = "1000"
            with self.assertRaisesRegex(ContractError, "second luminance scale"):
                validate_rendition(candidate)
        with self.assertRaisesRegex(ContractError, "float samples"):
            validate_rendition(media("linear", "u16"))

    def test_unresolved_rendering_is_not_a_complete_rendition(self):
        changes = [
            lambda m: m.update(gain_map={"base": "sdr", "map": "object"}),
            lambda m: m.update(composition="apng-subrectangles"),
            lambda m: m.update(scan="interlaced"),
            lambda m: m.update(orientation="rotate-90"),
            lambda m: m.update(alpha="premultiplied-linear"),
            lambda m: m["signal"].update(transfer="gain-map"),
        ]
        for change in changes:
            candidate = media()
            change(candidate)
            with self.assertRaises(ContractError):
                validate_rendition(candidate)

    def test_repacking_and_notes_do_not_change_media_identity(self):
        candidate = media()
        identity = rendition_id(candidate)
        plans = [{"segments": [segment()]}, {"segments": [segment("imi")]},
                 {"segments": [segment("png", 0, 1), segment("png", 1, 1)]}]
        plans[-1]["segments"][1]["location"] = {"embedded": "payload-1"}
        for plan in plans:
            validate_storage(candidate, plan)
            package = {"media": candidate, "storage": plan, "notes": "editable", "scores": []}
            package["notes"] = "changed without touching media"
            self.assertEqual(identity, rendition_id(package["media"]))

    def test_external_objects_need_hashes_but_embedded_hash_is_optional(self):
        plan = {"segments": [segment()]}
        validate_storage(media(), plan)
        s = plan["segments"][0]
        s["location"] = {"external": "https://example.invalid/clip.imi"}
        with self.assertRaisesRegex(ContractError, "exact content hash"):
            validate_storage(media(), plan)
        s["payload_sha256"] = "c" * 64
        validate_storage(media(), plan)

    def test_segment_coverage_and_type_are_checked(self):
        for s in (segment(first=1), segment(count=1), segment(count=3), segment(count=0),
                  segment("png"), segment("jpeg")):
            with self.assertRaises(ContractError):
                validate_storage(media(), {"segments": [s]})
        with self.assertRaisesRegex(ContractError, "overlap"):
            validate_storage(media(), {"segments": [segment("png", 0, 1), segment("png", 0, 1)]})
        with self.assertRaisesRegex(ContractError, "float"):
            validate_storage(media("linear", "f32"), {"segments": [segment()]})

    def test_timing_outside_apng_range_can_use_individual_png_frames(self):
        candidate = media()
        candidate["frames"][0]["duration"] = [1, 65536]
        candidate["frames"][1]["pts"] = [1, 65536]
        with self.assertRaisesRegex(ContractError, "exactly by APNG"):
            validate_storage(candidate, {"segments": [segment()]})
        validate_storage(candidate, {"segments": [segment("png", 0, 1), segment("png", 1, 1)]})
        validate_storage(candidate, {"segments": [segment("imi")]})

    def test_timeline_does_not_invent_gap_overlap_or_zero_duration_policy(self):
        for field, value in (("pts", [0, 1]), ("pts", [1, 1]), ("duration", [0, 1])):
            candidate = media()
            candidate["frames"][1][field] = value
            with self.assertRaises(ContractError):
                validate_rendition(candidate)

    def test_still_one_frame_sequence_and_looping_are_different(self):
        candidate = media()
        candidate["frames"] = candidate["frames"][:1]
        video = rendition_id(candidate)
        candidate["plays"] = 0
        self.assertNotEqual(video, rendition_id(candidate))
        candidate.update(kind="still", plays=None)
        candidate["frames"][0].update(pts=None, duration=None)
        self.assertNotEqual(video, rendition_id(candidate))
        validate_storage(candidate, {"segments": [segment("png", 0, 1)]})


if __name__ == "__main__":
    unittest.main()
