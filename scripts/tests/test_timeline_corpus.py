"""Keep campaign seeds reproducible and preserve existing failure inputs."""
import hashlib
import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).parents[1] / "seed-timeline-corpus.py"
spec = importlib.util.spec_from_file_location("timeline_corpus", SCRIPT)
corpus = importlib.util.module_from_spec(spec)
spec.loader.exec_module(corpus)


class TimelineCorpusTests(unittest.TestCase):
    def test_all_targets_have_repeatable_long_and_boundary_inputs(self):
        seeds = corpus.seeds()
        self.assertEqual(seeds, corpus.seeds())
        self.assertEqual(set(seeds), set(corpus.TARGETS))
        for inputs in seeds.values():
            self.assertIn(b"", inputs)
            self.assertTrue(any(len(data) >= 128 for data in inputs))
        for kind in range(2):
            for boundary in (0, 7, 15):
                for mutation in range(16):
                    self.assertIn(bytes([15, kind, boundary, mutation, 0]), seeds["replay_plan"])
                for failure in range(6):
                    for offset in (0, 7, 16):
                        self.assertTrue(any(data[:7] == bytes([15, 9, kind, boundary, 219, failure, offset])
                                            for data in seeds["correction"]))

    def test_seeding_is_idempotent_and_never_removes_mutated_inputs(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            def run():
                return subprocess.run([sys.executable, str(SCRIPT), "--root", str(root)],
                                      capture_output=True, text=True, check=True)
            run()
            for name, inputs in corpus.seeds().items():
                directory = root / f"fuzz_timeline_{name}"
                self.assertEqual(len(list(directory.iterdir())), len(inputs))
                for data in inputs:
                    self.assertEqual((directory / ("penelope-v1-" + hashlib.sha256(data).hexdigest())).read_bytes(), data)
            mutated = root / "fuzz_timeline_effects" / "existing-crash"
            mutated.write_bytes(b"preserve me")
            self.assertEqual(run().stdout.count("(0 added)"), len(corpus.TARGETS))
            self.assertEqual(mutated.read_bytes(), b"preserve me")

    def test_conflicting_seed_file_fails_without_overwriting(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            directory = root / "fuzz_timeline_projection"
            directory.mkdir()
            path = directory / ("penelope-v1-" + hashlib.sha256(b"").hexdigest())
            path.write_bytes(b"unexpected bytes")
            result = subprocess.run([sys.executable, str(SCRIPT), "--root", str(root)],
                                    capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("conflicting deterministic seed", result.stderr)
            self.assertEqual(path.read_bytes(), b"unexpected bytes")


if __name__ == "__main__":
    unittest.main()
