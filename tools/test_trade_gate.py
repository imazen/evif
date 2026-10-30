import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import trade_gate as g


def scores(s=90.0, b=1.0, c=9.9, e=80.0, **extra):
    out = {"ssim2-frozen": s, "butteraugli": b, "cvvdp": c, "zensim-e": e, "luma601-psnr": 40.0,
           "lumrel-psnr": 40.0, "chroma601-psnr": 40.0}
    out.update(extra)
    return out


class Truth(unittest.TestCase):
    def test_two_distinct_references_decide(self):
        a = scores()
        # S and B say worse, C abstains (change below its margin).
        v = g.votes(a, scores(s=80.0, b=3.0, c=9.8))
        self.assertEqual(v, {"ssim2-frozen": "worse", "butteraugli": "worse", "cvvdp": "abstain"})
        self.assertEqual(g.truth(v)[0], "WORSE")
        # B and C agree while S says better: still WORSE, S recorded as the dissenter.
        v = g.votes(a, scores(s=95.0, b=3.0, c=8.0))
        result, agreeing = g.truth(v)
        self.assertEqual((result, agreeing), ("WORSE", ["butteraugli", "cvvdp"]))

    def test_one_reference_never_decides(self):
        a = scores()
        for b in (scores(s=50.0), scores(b=9.0), scores(c=5.0)):
            self.assertEqual(g.truth(g.votes(a, b))[0], "UNDECIDED")

    def test_margins_are_strict(self):
        a = scores()
        # exactly the margin abstains; just beyond votes
        self.assertEqual(g.votes(a, scores(s=89.0))["ssim2-frozen"], "abstain")
        self.assertEqual(g.votes(a, scores(s=88.99))["ssim2-frozen"], "worse")
        self.assertEqual(g.votes(a, scores(b=1.5))["butteraugli"], "abstain")
        self.assertEqual(g.votes(a, scores(b=1.51))["butteraugli"], "worse")
        self.assertEqual(g.votes(a, scores(c=9.65))["cvvdp"], "abstain")
        self.assertEqual(g.votes(a, scores(c=9.64))["cvvdp"], "worse")
        # a better outcome mirrors it
        self.assertEqual(g.votes(a, scores(b=0.4))["butteraugli"], "better")

    def test_inversions(self):
        a = scores(e=80.0)
        worse = scores(s=70.0, b=4.0, e=80.0)  # references agree it is worse, model ties
        t, _ = g.truth(g.votes(a, worse))
        self.assertEqual(t, "WORSE")
        self.assertEqual(g.model_inversions(a, worse, t, ["zensim-e"]), {"zensim-e": "inversion"})
        lower = scores(s=70.0, b=4.0, e=79.9)
        self.assertEqual(g.model_inversions(a, lower, t, ["zensim-e"]), {"zensim-e": "ok"})
        better = scores(s=95.0, b=0.2, e=79.0)
        tb, _ = g.truth(g.votes(a, better))
        self.assertEqual(tb, "BETTER")
        self.assertEqual(g.model_inversions(a, better, tb, ["zensim-e"]), {"zensim-e": "reverse"})
        self.assertEqual(g.model_inversions(a, a, "UNDECIDED", ["zensim-e"]), {})


class Ladders(unittest.TestCase):
    def test_admission_walk_matches_the_chroma_only_rule(self):
        # a small drop is skipped, not a stop; the first adjacent tie or increase stops the walk
        kept, stop = g.admission([95, 94.5, 92.5, 92.4, 92.4, 80])
        self.assertEqual((kept, stop), ([0, 2], 4))
        self.assertEqual(g.admission([95, 90, 80]), ([0, 1, 2], None))
        self.assertEqual(g.admission([95, 96, 80]), ([0], 1))

    def test_trade_pair_needs_luma_up_and_chroma_down(self):
        a = {"scores": scores(), "bytes": 10}
        b = {"scores": scores(**{"luma601-psnr": 40.5, "chroma601-psnr": 39.0}), "bytes": 11}
        self.assertTrue(g.trade_pair(a, b)[0])
        # lumrel alone may carry the luma gain (XYB)
        b2 = {"scores": scores(**{"lumrel-psnr": 40.1, "chroma601-psnr": 39.0}), "bytes": 11}
        self.assertTrue(g.trade_pair(a, b2)[0])
        # below the 0.02 dB step, or chroma not falling
        b3 = {"scores": scores(**{"luma601-psnr": 40.01, "chroma601-psnr": 39.0}), "bytes": 11}
        self.assertFalse(g.trade_pair(a, b3)[0])
        b4 = {"scores": scores(**{"luma601-psnr": 41.0}), "bytes": 11}
        self.assertFalse(g.trade_pair(a, b4)[0])

    def row(self, rung, **kw):
        s = scores(**kw)
        return dict(kind="ladder", view="v", source="src", partition="gauntlet", trade_family="ycc420", ramp="large",
                    rung=rung, rungs=3, chroma=float(rung), scores=s, bytes=100 + rung, encoded_sha256=str(rung))

    def test_analyse_counts_an_inversion_on_a_decided_worse_trade_pair(self):
        rows = [self.row(0), self.row(1, s=70.0, b=4.0, e=81.0, **{"luma601-psnr": 41.0, "chroma601-psnr": 35.0}),
                self.row(2, s=71.0, b=8.0, e=70.0, **{"luma601-psnr": 42.0, "chroma601-psnr": 30.0})]
        result = g.analyse(iter(rows), ["zensim-e"])
        tally = g.tallies_json(result, ["zensim-e"])["gauntlet"]["a"]["zensim-e"]["src"]
        # rung 0->1: references worse (S, B), model higher: inversion. rung 1->2: B worse only: undecided.
        self.assertEqual(tally["pairs"], 2)
        self.assertEqual(tally["n_worse"], 1)
        self.assertEqual(tally["inv"], 1)
        self.assertEqual(tally["undecided"], 1)

    def test_de_finds_deduplicate_by_encoded_bytes_and_count_refusals_in_the_budget(self):
        base = dict(kind="de", view="v", source="src", partition="gauntlet", trade_family="ycc420", target="zensim-e",
                    arm="de", scores=scores(), bytes=1, find=True, legacy_find=False, margin=0.7)
        rows = [dict(base, index=0, encoded_sha256="x"), dict(base, index=1, encoded_sha256="x"),
                dict(base, index=2, encoded_sha256="y", find=False),
                dict(kind="de", view="v", source="src", partition="gauntlet", trade_family="ycc420",
                     target="zensim-e", arm="de", refused="no")]
        result = g.analyse(iter(rows), ["zensim-e"])
        b = g.tallies_json(result, ["zensim-e"])["gauntlet"]["b"]["zensim-e"]["src"]
        self.assertEqual((b["encodes"], b["finds"]), (4, 1))


if __name__ == "__main__":
    unittest.main()
