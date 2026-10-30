"""Benchmark evidence must reject output it cannot interpret reliably."""
import importlib.util
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


if __name__ == "__main__":
    unittest.main()
