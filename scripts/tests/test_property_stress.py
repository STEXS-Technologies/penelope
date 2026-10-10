"""Check timed campaign admission, duration and failure handling without waits."""
import importlib.util
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("property_stress", Path(__file__).parents[1] / "run_property_stress.py")
campaign = importlib.util.module_from_spec(spec)
spec.loader.exec_module(campaign)


class PropertyStressTests(unittest.TestCase):
    def test_each_batch_rotates_seeds_until_the_duration_is_met(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(campaign.time, "monotonic", side_effect=[0, 0, 0.1, 0.2, 5.1, 5.2]):
                with patch.object(campaign.subprocess, "run", return_value=SimpleNamespace(returncode=0)) as run:
                    evidence = campaign.exercise("binary", "property", 5, 100, Path(directory))
        self.assertGreaterEqual(evidence["elapsed_seconds"], 5)
        self.assertEqual(evidence["batches"], 3)
        self.assertEqual(evidence["requested_cases"], 300)
        self.assertEqual([call.kwargs["env"]["PROPTEST_RNG_SEED"] for call in run.call_args_list], ["1", "2", "3"])
        for call in run.call_args_list:
            self.assertEqual(call.args[0], ["binary", "--exact", "property", "--test-threads=1", "--nocapture"])
            self.assertEqual(call.kwargs["env"]["PROPTEST_CASES"], "100")

    def test_failure_stops_the_campaign_at_the_failing_seed(self):
        with tempfile.TemporaryDirectory() as directory:
            with patch.object(campaign.time, "monotonic", return_value=0):
                with patch.object(campaign.subprocess, "run", return_value=SimpleNamespace(returncode=1)) as run:
                    with self.assertRaisesRegex(RuntimeError, "failed at seed 1"):
                        campaign.exercise("binary", "property", 5, 100, Path(directory))
        self.assertEqual(run.call_count, 1)

    def test_missing_exact_test_cannot_pass_without_running_cases(self):
        artifacts = [{"reason": "compiler-artifact", "target": {"name": target}, "executable": target}
                     for target, _ in campaign.PROPERTIES]
        build = SimpleNamespace(stdout="\n".join(campaign.json.dumps(artifact) for artifact in artifacts))
        empty_listing = SimpleNamespace(stdout="0 tests, 0 benchmarks\n")
        with patch.object(campaign.subprocess, "run", side_effect=[build, empty_listing]):
            with self.assertRaisesRegex(RuntimeError, "property test not registered"):
                campaign.build_tests()

    def test_nonpositive_durations_and_case_counts_are_rejected(self):
        for value in ["0", "-1"]:
            with self.assertRaises(campaign.argparse.ArgumentTypeError):
                campaign.positive(value)


if __name__ == "__main__":
    unittest.main()
