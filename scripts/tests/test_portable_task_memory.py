"""The evidence reader must reject missing/invalid stack instrumentation."""
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location(
    "portable_memory", Path(__file__).resolve().parents[1] / "measure-portable-task-memory.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def profile(*values):
    return "desc: --stacks=yes --heap=no\n" + "".join(
        f"snapshot={index}\nmem_heap_B=0\nmem_heap_extra_B=0\nmem_stacks_B={value}\n"
        for index, value in enumerate(values))


class StackSamples(unittest.TestCase):
    def test_reads_maximum_including_nonfinal_snapshot(self):
        self.assertEqual(MODULE.stack_samples(profile(0, 9000, 12)),
                         {"maximum_sampled_stack_bytes": 9000, "snapshots": 3})

    def test_rejects_missing_and_zero_samples(self):
        for text in ("", "desc: --stacks=yes --heap=no\n", profile(0), profile(0, 0)):
            with self.subTest(text=text), self.assertRaises(ValueError):
                MODULE.stack_samples(text)

    def test_rejects_wrong_profile(self):
        with self.assertRaises(ValueError):
            MODULE.stack_samples(profile(0, 1).replace("--heap=no", "--heap=yes"))

    def test_rejects_missing_or_duplicate_counter(self):
        text = profile(0, 1)
        for damaged in (text.replace("mem_stacks_B=1\n", ""),
                        text + "mem_stacks_B=1\n"):
            with self.subTest(text=damaged), self.assertRaises(ValueError):
                MODULE.stack_samples(damaged)

    def test_rejects_negative_and_non_numeric(self):
        for value in (-1, "invalid"):
            with self.subTest(value=value), self.assertRaises(ValueError):
                MODULE.stack_samples(profile(0, value))

    def test_rejects_sequence_and_heap_mismatch(self):
        text = profile(0, 1)
        for damaged in (text.replace("snapshot=1", "snapshot=3"),
                        text.replace("mem_heap_B=0", "mem_heap_B=5"),
                        text.replace("mem_heap_extra_B=0", "mem_heap_extra_B=5")):
            with self.subTest(text=damaged), self.assertRaises(ValueError):
                MODULE.stack_samples(damaged)


if __name__ == "__main__":
    unittest.main()
