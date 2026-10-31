"""Reject damaged stack evidence and keep hosted allocation code out of it."""
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location(
    'task_stack_usage', Path(__file__).resolve().parents[1] / 'record-task-stack-usage.py')
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class TaskStackEvidence(unittest.TestCase):
    def test_preserves_dynamic_classification_instead_of_claiming_a_bound(self):
        rows = MODULE.parse_usage('unit:_Rexample\t24\tstatic\nunit:_Ropaque\t16\tdynamic\n')
        self.assertEqual(rows[0]['frame_bytes'], 24)
        self.assertEqual(rows[1]['classification'], 'dynamic')

    def test_rejects_truncated_and_negative_compiler_records(self):
        for text in ('unit:_Rexample\t24\n', '_Rexample\t24\tstatic\n',
                     'unit:_Rexample\t-1\tstatic\n', 'unit:\t0\tstatic\n'):
            with self.subTest(text=text), self.assertRaises(ValueError):
                MODULE.parse_usage(text)

    def test_generated_fixture_retains_fixed_ports_but_no_hosted_allocator_or_main(self):
        original = (MODULE.ROOT / 'crates/penelope-core/tests/task_allocations.rs').read_text()
        source = MODULE.fixture_source(original)
        self.assertIn('impl TaskJournal for Journal', source)
        self.assertIn('impl TaskRuntime for Runtime', source)
        self.assertIn('fn stack_positive_control()', source)
        for forbidden in ('GlobalAlloc', 'System.alloc', 'std::', 'fn main()'):
            self.assertNotIn(forbidden, source)
        with self.assertRaises(ValueError):
            MODULE.fixture_source(original.replace('fn stack_positive_control()', 'fn moved_control()'))


if __name__ == '__main__':
    unittest.main()
