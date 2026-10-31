"""Reject invalid workloads, allocation evidence and process-resource records."""
import importlib.util
import json
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location(
    "hosted_memory", Path(__file__).resolve().parents[1] / "measure-hosted-task-memory.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class HostedMemory(unittest.TestCase):
    def setUp(self):
        self.value = {"capacity": 4096, "repetitions": 10000, "allocations": 0,
                      "reallocations": 0, "deallocations": 0, "requested_bytes": 0,
                      "supervisor_bytes": 13656, "zero_sized_ports_supervisor_bytes": 416}

    def test_accepts_exact_workload_and_layout(self):
        self.assertEqual(MODULE.validate_output(json.dumps(self.value), 4096, 10000), self.value)

    def test_rejects_nonzero_allocations_and_wrong_workload(self):
        for key in ("capacity", "repetitions", "allocations", "reallocations",
                    "deallocations", "requested_bytes"):
            with self.subTest(key=key), self.assertRaises(ValueError):
                MODULE.validate_output(json.dumps({**self.value, key: self.value[key] + 1}), 4096, 10000)

    def test_rejects_missing_extra_and_untyped_fields(self):
        for value in ({key: value for key, value in self.value.items() if key != "allocations"},
                      {**self.value, "unexpected": 1}, {**self.value, "allocations": False},
                      {**self.value, "supervisor_bytes": 0}, {**self.value, "repetitions": "10000"}):
            with self.subTest(value=value), self.assertRaises(ValueError):
                MODULE.validate_output(json.dumps(value), 4096, 10000)

    def test_rejects_nonfinite_negative_missing_and_zero_rss(self):
        for value in ("nan 0 0 1", "0 inf 0 1", "-1 0 0 1", "0 0 0 0", "0 0 0", "0 0 0 x"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                MODULE.native_resources(value)
        self.assertEqual(MODULE.native_resources("0.1 0.2 0 4096")["peak_rss_kib"], 4096)

    def test_capacity_substitution_keeps_hosted_fixture(self):
        value = MODULE.fixture_source(65536)
        self.assertIn("const CAPACITY: usize = 65536;", value)
        self.assertIn("TaskSupervisor<Journal, Runtime, CAPACITY>", value)
        with self.assertRaises(ValueError):
            MODULE.fixture_source(7)


if __name__ == "__main__":
    unittest.main()
