"""Ensure retained latency summaries agree with raw samples and timing order."""
import copy
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location(
    "worker_dispatch", Path(__file__).resolve().parents[1] / "benchmark-worker-dispatch.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def fixture():
    profiles = []
    for reused, name in enumerate(MODULE.PROFILES):
        rows = []
        for index in range(2 + reused):
            warmup = bool(reused and index == 0)
            scale = 1000 if warmup else index + 1
            rows.append({"index": index, "warmup": warmup,
                         **{field: (ordinal + 1) * scale
                            for ordinal, field in enumerate(MODULE.FIELDS)}})
        measured = rows[1:] if reused else rows
        distributions = {}
        for field in MODULE.FIELDS:
            values = sorted(row[field] for row in measured)
            distributions[field] = {"samples": 2, "p50_ns": values[0],
                                    "p95_ns": values[1], "p99_ns": values[1]}
        profiles.append({"profile": name, "samples": rows, "distributions": distributions})
    return {"schema": 1, "journal_mode": "WAL", "synchronous": "FULL",
            "worker_input": [5, 1, 2, 3], "worker_output_u64": 6,
            "worker_code_sha256": [0] * 32, "profiles": profiles}


class WorkerDispatchEvidence(unittest.TestCase):
    def test_valid_reports_exclude_warmup_from_percentiles(self):
        MODULE.validate_report(fixture(), 2)

    def test_rejects_wrong_profile_or_code(self):
        for key, value in (("journal_mode", "DELETE"), ("synchronous", "OFF"),
                           ("worker_input", [0]), ("worker_output_u64", 7),
                           ("worker_code_sha256", [0] * 31)):
            report = fixture()
            report[key] = value
            with self.subTest(key=key), self.assertRaises(ValueError):
                MODULE.validate_report(report, 2)

    def test_rejects_missing_or_substituted_sample(self):
        for damage in ("count", "index", "warmup"):
            report = fixture()
            rows = report["profiles"][1]["samples"]
            if damage == "count":
                rows.pop()
            elif damage == "index":
                rows[1]["index"] = 99
            else:
                rows[1]["warmup"] = True
            with self.subTest(damage=damage), self.assertRaises(ValueError):
                MODULE.validate_report(report, 2)

    def test_rejects_nan_negative_and_reversed_endpoint(self):
        for value in (float("nan"), -1, 1):
            report = fixture()
            report["profiles"][0]["samples"][0]["dispatch_to_host_checkpoint_ack_ns"] = value
            with self.subTest(value=value), self.assertRaises(ValueError):
                MODULE.validate_report(report, 2)

    def test_rejects_percentiles_that_include_warmup_or_change_rank(self):
        for value in (5000, 11):
            report = fixture()
            report["profiles"][1]["distributions"][MODULE.FIELDS[-1]]["p99_ns"] = value
            with self.subTest(value=value), self.assertRaises(ValueError):
                MODULE.validate_report(report, 2)

    def test_rejects_missing_distribution_and_reordered_profiles(self):
        first = fixture()
        first["profiles"][0]["distributions"].pop(MODULE.FIELDS[0])
        second = copy.deepcopy(fixture())
        second["profiles"].reverse()
        for report in (first, second):
            with self.assertRaises(ValueError):
                MODULE.validate_report(report, 2)


if __name__ == "__main__":
    unittest.main()
