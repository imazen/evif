"""Counterexamples for the evaluation design, not measured IQA results."""
from copy import deepcopy
from fractions import Fraction
import hashlib
import json
from pathlib import Path
import subprocess
import unittest

from model import (ContractError, asset_id, canonical, delivery_pairs, duration_mean,
                   evaluation_id, validate, validate_result)

EXAMPLE = Path(__file__).with_name("example.json")


def example():
    return json.loads(EXAMPLE.read_text())


class ContractTests(unittest.TestCase):
    def test_example(self):
        validate(example())

    def test_identity_covers_interpretation_not_only_metric_name(self):
        e = example()
        baseline = evaluation_id(e)
        mutations = [
            lambda x: x["metric"].update(implementation="different-build"),
            lambda x: x["metric"].update(model_sha256="f" * 64),
            lambda x: x["metric"]["config"].update(temp_filter="grad_trans"),
            lambda x: x["inputs"]["candidate"]["color"].update(transfer="hlg"),
            lambda x: x["inputs"]["candidate"]["frames"][0].update(samples_sha256="f" * 64),
            lambda x: x["pipeline"].update(boundary="symmetric"),
            lambda x: x["pipeline"]["context_frames"].update(before=2),
            lambda x: x["pipeline"]["candidate"][0].update(implementation="different-converter"),
            lambda x: x["output"].update(key="another-output"),
        ]
        ids = [baseline]
        for change in mutations:
            x = deepcopy(e)
            change(x)
            ids.append(evaluation_id(x))
        self.assertEqual(len(ids), len(set(ids)))

    def test_resolved_viewing_conditions_change_identity(self):
        e = example()
        changed = deepcopy(e)
        changed["view"]["display"]["peak_nits"] = "600"
        changed["metric"]["config"]["peak_nits"] = "600"
        self.assertNotEqual(evaluation_id(e), evaluation_id(changed))

    def test_requested_effective_condition_mismatch_is_refused(self):
        e = example()
        e["view"]["display"]["peak_nits"] = "600"
        with self.assertRaisesRegex(ContractError, "effective conditions"):
            validate(e)

    def test_display_profile_name_is_not_a_snapshot(self):
        e = example()
        e["view"] = {"status": "specified", "profile": "standard_hdr_pq"}
        with self.assertRaises(ContractError):
            validate(e)

    def test_metric_name_does_not_limit_model_assumptions(self):
        e = example()
        e["metric"].update(family="vmaf", temporal="stateful",
                           view_binding={"mode": "model-assumed", "parameters": {},
                                         "assumptions": {"distance_h": "3", "signal": "sdr"}})
        e["output"].update(key="vmaf", kind="quality", units="model-scale")
        validate(e)
        e["metric"]["view_binding"]["assumptions"] = {}
        with self.assertRaisesRegex(ContractError, "assumptions"):
            validate(e)

    def test_unknown_conditions_are_not_implicit_sdr_defaults(self):
        e = example()
        e["view"] = {"status": "unknown"}
        with self.assertRaisesRegex(ContractError, "resolved conditions"):
            validate(e)
        e["metric"]["view_binding"] = {"mode": "not-modeled", "parameters": {}, "assumptions": {}}
        validate(e)

    def test_repacking_changes_neither_media_nor_evaluation_identity(self):
        e = example()
        a = {"evaluation": e, "storage": {"codec": "apng", "path": "a.png", "bits": True}}
        b = deepcopy(a)
        b["storage"] = {"codec": "imij", "path": "b.imi", "bits": False}
        self.assertEqual(evaluation_id(a["evaluation"]), evaluation_id(b["evaluation"]))
        self.assertEqual(asset_id(a["evaluation"]["inputs"]["candidate"]), asset_id(b["evaluation"]["inputs"]["candidate"]))

    def test_vfr_to_cfr_is_an_explicit_presentation_time_map(self):
        e = example()
        frames = e["inputs"]["candidate"]["frames"]
        frames[0].update(pts=[0, 1], duration=[1, 2])
        frames[1].update(pts=[1, 2], duration=[1, 4])
        frames[2].update(pts=[3, 4], duration=[1, 4])
        for i, f in enumerate(e["inputs"]["reference"]["frames"]):
            f.update(pts=[i, 3] if i else [0, 1], duration=[1, 3])
        e["pipeline"].update(alignment="presentation-hold", delivery={"mode": "cfr-hold", "rate": [4, 1], "phase": [0, 1]})
        self.assertEqual(delivery_pairs(e), [(0, 0), (0, 0), (1, 1), (2, 2)])
        e["pipeline"]["delivery"]["phase"] = [1, 8]
        self.assertEqual(delivery_pairs(e), [(0, 0), (1, 0), (1, 1), (2, 2)])

    def test_frame_index_alignment_cannot_hide_dropped_time(self):
        e = example()
        e["inputs"]["candidate"]["frames"].pop()
        with self.assertRaisesRegex(ContractError, "identical timelines"):
            validate(e)
        e["pipeline"]["alignment"] = "presentation-hold"
        with self.assertRaisesRegex(ContractError, "truncate"):
            validate(e)

    def test_equal_spans_do_not_make_native_frame_indices_equivalent(self):
        e = example()
        frames = e["inputs"]["reference"]["frames"]
        frames[0].update(duration=[1, 16])
        frames[1].update(pts=[1, 16], duration=[1, 48])
        e["pipeline"]["alignment"] = "presentation-hold"
        with self.assertRaisesRegex(ContractError, "explicit delivery map"):
            delivery_pairs(e)

    def test_overlap_gap_zero_duration_and_noncanonical_time_refused(self):
        for field, value in [("pts", [0, 1]), ("pts", [1, 1]), ("duration", [0, 1]), ("duration", [2, 48]), ("duration", [1, 0])]:
            e = example()
            e["inputs"]["candidate"]["frames"][1][field] = value
            with self.assertRaises(ContractError):
                validate(e)

    def test_native_temporal_adapter_does_not_invent_a_vfr_rate(self):
        e = example()
        for asset in e["inputs"].values():
            asset["frames"][2]["duration"] = [1, 12]
        with self.assertRaisesRegex(ContractError, "VFR"):
            validate(e)

    def test_duration_pooling_does_not_equal_frame_averaging(self):
        values = [0, 100]
        self.assertEqual(duration_mean(values, [[9, 10], [1, 10]]), 10)
        self.assertNotEqual(duration_mean(values, [[9, 10], [1, 10]]), sum(values)/len(values))
        with self.assertRaises(ContractError):
            duration_mean([0], [[1, 1], [1, 1]])

    def test_stateless_clip_score_requires_named_wrapper_pooling(self):
        e = example()
        e["metric"]["temporal"] = "stateless"
        with self.assertRaisesRegex(ContractError, "native clip quality"):
            validate(e)
        e["output"]["pooling"] = {"owner": "wrapper", "method": "duration-mean"}
        validate(e)

    def test_pooled_scorer_cannot_fabricate_per_frame_scores(self):
        e = example()
        e["output"]["scope"] = "frame"
        with self.assertRaisesRegex(ContractError, "per-frame"):
            validate(e)

    def test_feature_and_no_reference_outputs_are_representable(self):
        e = example()
        e["metric"].update(temporal="stateless", reference_mode="no-reference")
        e["inputs"]["reference"] = None
        e["output"].update(kind="feature", direction="none", scope="frame", pooling={"owner": "none", "method": "none"})
        validate(e)
        e["output"]["direction"] = "higher"
        with self.assertRaisesRegex(ContractError, "quality target"):
            validate(e)

    def test_still_has_no_invented_fps_and_one_frame_video_is_distinct(self):
        e = example()
        for a in e["inputs"].values():
            a["frames"] = a["frames"][:1]
        video_id = evaluation_id(e)
        e["metric"]["temporal"] = "stateless"
        e["pipeline"]["delivery"] = {"mode": "still"}
        for a in e["inputs"].values():
            a["kind"] = "still"
            a["frames"][0].update(pts=None, duration=None)
        self.assertNotEqual(video_id, evaluation_id(e))

    def test_incomplete_coverage_is_not_a_complete_score(self):
        e = example()
        result = {"evaluation_id": evaluation_id(e), "status": "complete", "requested": [0, 1, 2], "completed": [0, 1], "value": {"finite": "9"}}
        with self.assertRaisesRegex(ContractError, "coverage"):
            validate_result(e, result)
        result.update(status="partial", value=None)
        validate_result(e, result)

    def test_delayed_results_keep_frame_identity_and_cannot_duplicate_coverage(self):
        e = example()
        result = {"evaluation_id": evaluation_id(e), "status": "complete", "requested": [0, 1, 2], "completed": [1, 0, 2], "value": {"finite": "9"}}
        validate_result(e, result)
        for completed in ([0, 0, 2], [0, 1, 3]):
            result["completed"] = completed
            with self.assertRaisesRegex(ContractError, "duplicate or unknown"):
                validate_result(e, result)

    def test_nonfinite_scores_are_typed_not_json_nan_or_zero(self):
        e = example()
        r = {"evaluation_id": evaluation_id(e), "status": "complete", "requested": [0, 1, 2], "completed": [0, 1, 2], "value": {"special": "+inf"}}
        with self.assertRaisesRegex(ContractError, "nonfinite"):
            validate_result(e, r)
        e["output"]["kind"] = "decibels"
        r["evaluation_id"] = evaluation_id(e)
        validate_result(e, r)
        r["value"] = {"finite": "NaN"}
        with self.assertRaises(ContractError):
            validate_result(e, r)

    def test_canonical_subset_matches_independent_javascript_serialization(self):
        value = {"z": [None, True, 9007199254740991, "é\n\t\u2028"], "a": {"b": 0, "a": "-0.125"}}
        code = "const fs=require('fs');const v=JSON.parse(fs.readFileSync(0,'utf8'));const s=x=>Array.isArray(x)?x.map(s):x&&typeof x==='object'?Object.fromEntries(Object.keys(x).sort().map(k=>[k,s(x[k])])):x;process.stdout.write(JSON.stringify(s(v)));"
        actual = subprocess.check_output(["node", "-e", code], input=json.dumps(value).encode())
        self.assertEqual(canonical(value), actual)
        self.assertEqual(hashlib.sha256(canonical(value)).digest(), hashlib.sha256(actual).digest())
        for invalid in [1.0, float("nan"), 2**53, {"é": 1}, "\ud800"]:
            with self.assertRaises(ContractError):
                canonical(invalid)


if __name__ == "__main__":
    unittest.main()
