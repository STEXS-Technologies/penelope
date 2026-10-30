"""Check actual cache residency and reject mislabeled replay evidence."""
import importlib.util
from pathlib import Path
import platform
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    "sqlite_cache", Path(__file__).resolve().parents[1] / "benchmark-sqlite-cache.py")
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class SqliteCacheEvidence(unittest.TestCase):
    @unittest.skipUnless(platform.system() == "Linux", "Linux mincore required")
    def test_real_owned_file_is_verified_cold_and_warm(self):
        with tempfile.TemporaryDirectory(prefix=".cache-probe-", dir=MODULE.ROOT) as temporary:
            path = Path(temporary) / "pages"
            path.write_bytes(bytes(range(256)) * 128)
            for mode in ("cold", "warm", "cold"):
                report = MODULE.set_cache(path, mode)
                state = report["verified_before_launch"]
                self.assertEqual(state["resident_pages"], 0 if mode == "cold" else state["total_pages"])
                self.assertGreater(state["total_pages"], 1)
            self.assertEqual(path.read_bytes(), bytes(range(256)) * 128)

    @unittest.skipUnless(platform.system() == "Linux", "Linux file checks required")
    def test_sidecar_and_symlink_do_not_receive_a_cache_label(self):
        with tempfile.TemporaryDirectory(prefix=".cache-probe-", dir=MODULE.ROOT) as temporary:
            path = Path(temporary) / "pages"
            path.write_bytes(b"x" * 8192)
            Path(str(path) + "-wal").write_bytes(b"uncheckpointed")
            with self.assertRaises(ValueError):
                MODULE.set_cache(path, "cold")
            link = Path(temporary) / "link"
            link.symlink_to(path)
            with self.assertRaises(OSError):
                MODULE.set_cache(link, "warm")

    def test_rejects_wrong_dataset_and_false_durability_or_timing(self):
        valid = {"schema": 1, "records": 16, "journal_mode": "WAL",
                 "synchronous": "FULL", "open_and_replay_ns": 123}
        MODULE.validate_report(valid, 16)
        for key, value in (("records", 512), ("journal_mode", "DELETE"),
                           ("synchronous", "OFF"), ("open_and_replay_ns", -1),
                           ("open_and_replay_ns", float("nan")), ("open_and_replay_ns", True)):
            with self.subTest(key=key, value=value), self.assertRaises(ValueError):
                MODULE.validate_report({**valid, key: value}, 16)


if __name__ == "__main__":
    unittest.main()
