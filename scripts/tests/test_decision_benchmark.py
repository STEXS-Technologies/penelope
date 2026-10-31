"""Benchmark evidence must reject output it cannot interpret reliably."""
import importlib.util
import json
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location(
    "decision_benchmark", Path(__file__).resolve().parents[1] / "benchmark-decision-comparison.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class BenchmarkObservations(unittest.TestCase):
    def test_serial_parallel_and_replay_units(self):
        samples = [
            ("linear-saga: 100 operations in 20µs (200 ns/op)", 200),
            ("linear-saga replay: 1000 logs in 3ms (3000 ns/replay)", 3000),
            ("parallel graph-saga: 400 isolated operations across 4 workers in 2ms (200000 ops/s)", 5000),
            ("encoding: 100 operations in 100ns (1 ns/op, 100 bytes)", 1),
            ("encoding: 100 operations in 1s (10000000 ns/op)", 10000000),
        ]
        for text, expected in samples:
            with self.subTest(text=text):
                result = MODULE.observations(text)
                self.assertEqual(len(result), 1)
                self.assertEqual(result[0]["ns_per_operation"], expected)

    def test_multiple_operations_remain_separate(self):
        result = MODULE.observations("encoding: 100 operations in 10µs\nappend: 10 operations in 1ms")
        self.assertEqual([item["operation"] for item in result], ["encoding", "append"])
        self.assertEqual([item["operations"] for item in result], [100, 10])

    def test_missing_zero_and_unknown_measurements_are_errors(self):
        for output in ["", "warning", "test: 0 operations in 1s", "test: 1 operations in 0ns",
                       "test: 10 operations in 5days", "test: 10 operations in NaNs"]:
            with self.subTest(output=output), self.assertRaises(ValueError):
                MODULE.observations(output)


class PerformanceCounters(unittest.TestCase):
    def records(self):
        return [{"event": event, "counter-value": "0.000000", "pcnt-running": 100}
                for event in MODULE.PERF_EVENTS]

    def test_exact_complete_counters_preserve_zero_values(self):
        records = self.records()
        wire = "\n".join(json.dumps(item) for item in records)
        self.assertEqual(MODULE.performance_counters(wire), records)

    def test_missing_duplicate_and_unavailable_counters_fail(self):
        invalid = [self.records()[:-1], self.records() + self.records()[:1]]
        duplicated = self.records()
        duplicated[-1]["event"] = duplicated[0]["event"]
        invalid.append(duplicated)
        for value in ["<not supported>", "nan", "inf", "-1"]:
            records = self.records()
            records[0]["counter-value"] = value
            invalid.append(records)
        for value in [0, -1, 101]:
            records = self.records()
            records[0]["pcnt-running"] = value
            invalid.append(records)
        for records in invalid:
            with self.subTest(records=records), self.assertRaises(ValueError):
                MODULE.performance_counters("\n".join(json.dumps(item) for item in records))


if __name__ == "__main__":
    unittest.main()
