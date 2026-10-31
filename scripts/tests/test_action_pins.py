"""The release action-pin gate must scan successfully without ripgrep."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'check-action-pins.sh'


class ActionPins(unittest.TestCase):
    def run_gate(self, directory, content=None):
        root = Path(directory)
        tools = root / 'tools'
        tools.mkdir()
        for name in ('bash', 'grep'):
            (tools / name).symlink_to(shutil.which(name))
        workflows = root / 'workflows'
        if content is not None:
            workflows.mkdir()
            (workflows / 'ci.yml').write_text(content)
        return subprocess.run([str(SCRIPT), str(workflows)],
                              env={**os.environ, 'PATH': str(tools)},
                              text=True, capture_output=True)

    def test_valid_pinned_reference_without_rg(self):
        with tempfile.TemporaryDirectory() as directory:
            result = self.run_gate(directory, '  - uses: actions/checkout@' + 'a' * 40 + ' # pinned\n')
            self.assertEqual(result.returncode, 0, result.stderr)

    def test_mutable_reference_fails_without_rg(self):
        with tempfile.TemporaryDirectory() as directory:
            result = self.run_gate(directory, '  - uses: actions/checkout@v4\n')
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('mutable or malformed', result.stderr)

    def test_failed_scan_never_reports_success(self):
        with tempfile.TemporaryDirectory() as directory:
            result = self.run_gate(directory)
            self.assertNotEqual(result.returncode, 0)
            self.assertNotIn('all GitHub Actions are pinned', result.stdout)


if __name__ == '__main__':
    unittest.main()
