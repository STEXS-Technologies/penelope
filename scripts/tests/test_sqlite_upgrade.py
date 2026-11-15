import importlib.util
from pathlib import Path
import sqlite3
import tempfile
import unittest


spec = importlib.util.spec_from_file_location(
    "sqlite_upgrade", Path(__file__).resolve().parents[1] / "check-sqlite-upgrade.py"
)
upgrade = importlib.util.module_from_spec(spec)
spec.loader.exec_module(upgrade)


class UpgradeSnapshotTests(unittest.TestCase):
    def test_equal_row_counts_do_not_hide_rewritten_bytes_or_storage_types(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "test.sqlite"
            with sqlite3.connect(path) as connection:
                connection.execute("CREATE TABLE history(value)")
                connection.execute("INSERT INTO history VALUES (?)", (b"input",))
            before = upgrade.snapshot(path)
            with sqlite3.connect(path) as connection:
                connection.execute("UPDATE history SET value=?", ("input",))
            self.assertNotEqual(upgrade.snapshot(path), before)
            with sqlite3.connect(path) as connection:
                connection.execute("UPDATE history SET value=?", (b"other",))
            self.assertNotEqual(upgrade.snapshot(path), before)

    def test_new_tables_are_allowed_but_legacy_schema_changes_are_detected(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "test.sqlite"
            with sqlite3.connect(path) as connection:
                connection.execute("CREATE TABLE legacy(value BLOB)")
            before = upgrade.snapshot(path)
            with sqlite3.connect(path) as connection:
                connection.execute("CREATE TABLE machine(value BLOB)")
            self.assertEqual(upgrade.snapshot(path, before), before)
            with sqlite3.connect(path) as connection:
                connection.execute("ALTER TABLE legacy ADD COLUMN extra INTEGER")
            self.assertNotEqual(upgrade.snapshot(path, before), before)


if __name__ == "__main__":
    unittest.main()
