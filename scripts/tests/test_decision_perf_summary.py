"""Check paired analysis against known ratios and invalid samples."""
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location(
    "decision_perf", Path(__file__).resolve().parents[1] / "summarize-decision-perf.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class PairedStatistics(unittest.TestCase):
    def test_identical_and_fixed_ratio_pairs(self):
        for ratio, expected in [(1, 0), (2, 100), (0.5, -50)]:
            with self.subTest(ratio=ratio):
                data = MODULE.paired_statistics([1, 2, 4], [ratio, ratio * 2, ratio * 4], 100)
                self.assertAlmostEqual(data["paired_geometric_mean_delta_percent"], expected)
                for bound in data["paired_bootstrap_percentile_95_percent"]:
                    self.assertAlmostEqual(bound, expected)

    def test_seed_makes_variable_pair_analysis_repeatable(self):
        arguments = ([1, 2, 3], [2, 2, 2], 100, 3030)
        self.assertEqual(MODULE.paired_statistics(*arguments), MODULE.paired_statistics(*arguments))

    def test_invalid_samples_fail(self):
        for before, after, resamples in [([], [], 100), ([1], [1, 2], 100),
                ([0], [1], 100), ([1], [-1], 100), ([1], [float("nan")], 100),
                ([1], [float("inf")], 100), ([1], [1], 0)]:
            with self.subTest(before=before, after=after), self.assertRaises(ValueError):
                MODULE.paired_statistics(before, after, resamples)


if __name__ == "__main__":
    unittest.main()
