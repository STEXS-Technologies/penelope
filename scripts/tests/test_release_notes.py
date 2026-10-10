"""Release publication must select exactly one nonempty dated version section."""
import importlib.util
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location(
    "release_notes", Path(__file__).resolve().parents[1] / "release-notes.py"
)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ReleaseNotesTests(unittest.TestCase):
    def test_extracts_only_the_tagged_version_and_keeps_subsections(self):
        changelog = """# Changelog

## [Unreleased]
Future work.

## [0.2.0] - 2026-10-11
Recoverable tasks.

### Added
- Checkpoints.

## [0.1.0] - 2026-09-15
Sagas.
"""
        expected = "Recoverable tasks.\n\n### Added\n- Checkpoints.\n"
        self.assertEqual(module.release_notes(changelog, "v0.2.0"), expected)
        self.assertEqual(module.release_notes(changelog, "0.2.0"), expected)
        self.assertEqual(module.release_notes(changelog, "v0.1.0"), "Sagas.\n")

    def test_missing_or_undated_entry_is_rejected(self):
        for changelog in ["## [Unreleased]\nNotes.", "## [0.2.0]\nNotes."]:
            with self.assertRaises(ValueError):
                module.release_notes(changelog, "v0.2.0")

    def test_duplicate_version_entries_are_rejected(self):
        changelog = "## [0.2.0] - 2026-10-11\nOne.\n## [0.2.0] - 2026-10-10\nTwo."
        with self.assertRaises(ValueError):
            module.release_notes(changelog, "v0.2.0")

    def test_empty_entry_is_rejected_without_using_the_next_version(self):
        changelog = "## [0.2.0] - 2026-10-11\n\n## [0.1.0] - 2026-09-15\nOlder."
        with self.assertRaises(ValueError):
            module.release_notes(changelog, "v0.2.0")


if __name__ == "__main__":
    unittest.main()
