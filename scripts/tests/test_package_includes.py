import importlib.util
import io
from pathlib import Path
import tarfile
import tempfile
import unittest


spec = importlib.util.spec_from_file_location(
    "package_includes", Path(__file__).resolve().parents[1] / "check-package-includes.py"
)
checker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(checker)


class PackageIncludesTests(unittest.TestCase):
    def check(self, source, files=()):
        with tempfile.TemporaryDirectory() as temporary:
            archive = Path(temporary) / "test.crate"
            with tarfile.open(archive, "w:gz") as package:
                for name, data in [("sample-0.1.0/tests/test.rs", source), *files]:
                    encoded = data.encode()
                    member = tarfile.TarInfo(name)
                    member.size = len(encoded)
                    package.addfile(member, io.BytesIO(encoded))
            return checker.validate(archive)

    def test_materialized_shared_fixture_and_binary_asset(self):
        self.assertEqual(self.check(
            '#[path = "support/shared.rs"] mod fixture;\n'
            'const BYTES: &[u8] = include_bytes!("../data.bin");',
            [("sample-0.1.0/tests/support/shared.rs", "pub fn fixture() {}"),
             ("sample-0.1.0/data.bin", "bytes")],
        ), [])

    def test_workspace_neighbor_is_not_a_packaged_fixture(self):
        errors = self.check('#[path = "../../neighbor/tests/fixture.rs"] mod fixture;')
        self.assertEqual(len(errors), 1)
        self.assertIn("neighbor/tests/fixture.rs", errors[0])

    def test_missing_literal_macros_are_reported(self):
        self.assertEqual(len(self.check(
            'include!("generated.rs");\ninclude_str!("missing.txt");'
        )), 2)

    def test_another_archive_root_cannot_satisfy_an_escape(self):
        self.assertEqual(len(self.check(
            'include!("../../neighbor/file.rs");', [("neighbor/file.rs", "")]
        )), 1)


if __name__ == "__main__":
    unittest.main()
