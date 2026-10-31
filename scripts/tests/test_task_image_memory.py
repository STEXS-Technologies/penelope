"""Allocated image sections are distinct from stack and loader placement."""
import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location(
    'bound_runner', Path(__file__).resolve().parents[1] / 'bound-portable-task-stack.py')
RUNNER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUNNER)


def section(name, size, flags):
    return {'Section': {'Name': {'Name': name}, 'Size': size, 'Flags': {'Value': flags}}}


def report(sections):
    return [{'FileSummary': {'Format': 'elf32-littleriscv'}, 'Sections': sections}]


class TaskImageMemory(unittest.TestCase):
    def test_excludes_symbol_tables_and_separates_writable_bytes(self):
        result = RUNNER.image_sections(report([
            section('.text', 100, 6), section('.rodata', 30, 2),
            section('.data', 10, 3), section('.bss', 20, 3),
            section('.symtab', 9999, 0)]))
        self.assertEqual(result['writable_section_bytes'], 30)
        self.assertEqual(result['readonly_section_bytes'], 130)
        self.assertEqual(len(result['sections']), 4)

    def test_rejects_wrong_image_or_missing_executable(self):
        for value in [[], [{'FileSummary': {'Format': 'elf64-x86-64'}}],
                      report([section('.rodata', 32, 2)])]:
            with self.assertRaises(ValueError):
                RUNNER.image_sections(value)

    def test_rejects_invalid_measurements(self):
        for size, flags in [(-1, 6), (1, -1), ('16', 6)]:
            with self.assertRaises(ValueError):
                RUNNER.image_sections(report([section('.text', size, flags)]))


if __name__ == '__main__':
    unittest.main()
