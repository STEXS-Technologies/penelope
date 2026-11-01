"""Reject incomplete or misleading stack measurement output."""
import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('timeline_memory', Path(__file__).parents[1] / 'measure-timeline-memory.py')
memory = importlib.util.module_from_spec(spec)
spec.loader.exec_module(memory)


class TimelineMemoryTests(unittest.TestCase):
    def test_independent_stack_and_combined_maxima(self):
        result = memory.parse_massif('snapshot=0\nmem_heap_B=10\nmem_heap_extra_B=2\nmem_stacks_B=90\n'
                                     'snapshot=1\nmem_heap_B=100\nmem_heap_extra_B=10\nmem_stacks_B=20\n')
        self.assertEqual(result, {'snapshots': 2, 'maximum_sampled_stack_bytes': 90, 'maximum_sampled_total_bytes': 130})

    def test_missing_duplicate_and_negative_fields_fail(self):
        for text in ['', 'snapshot=0\nmem_stacks_B=10\n',
                     'snapshot=0\nmem_heap_B=0\nmem_heap_extra_B=0\nmem_stacks_B=-1\n',
                     'snapshot=0\nmem_heap_B=0\nmem_heap_extra_B=0\nmem_stacks_B=0\nmem_stacks_B=1\n']:
            with self.assertRaises(ValueError):
                memory.parse_massif(text)


if __name__ == '__main__':
    unittest.main()
