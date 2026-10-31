"""Read-only verifier for the pinned deferred process COMMIT failure audit."""
from pathlib import Path
import gzip
import hashlib
import json
import re
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]


def digest(data):
    return hashlib.sha256(data).hexdigest()


def log(name):
    return gzip.decompress((HERE / (name + '.txt.gz')).read_bytes()).decode()


def main():
    actual = {str(p.relative_to(HERE)): digest(p.read_bytes())
              for p in HERE.rglob('*') if p.is_file() and p.name != 'manifest.json'}
    assert json.loads((HERE / 'manifest.json').read_text()) == actual
    for name, expected in json.loads((HERE / 'source-hashes.json').read_text()).items():
        assert digest((ROOT / name).read_bytes()) == expected, name
    metadata = json.loads((HERE / 'metadata.json').read_text())
    path = 'crates/penelope-sqlite/src/process.rs'
    baseline = subprocess.check_output(['git', 'show', metadata['parent'] + ':' + path], cwd=ROOT)
    suffix = b'\n#[cfg(test)]\n#[path = "process_commit_boundary_tests.rs"]\nmod commit_boundary_tests;\n'
    assert (ROOT / path).read_bytes() == baseline + suffix, 'release implementation changed'
    total = 0
    expected_counts = {'deferred_atomic_commit_cases': 16, 'deferred_append_cases': 8, 'deferred_inbox_cases': 4}
    for toolchain in ['stable', 'msrv']:
        text = log(toolchain + '-repetitions')
        summary = json.loads((HERE / (toolchain + '-repetitions.json')).read_text())
        rows = summary['repetitions']
        assert [r['index'] for r in rows] == list(range(20))
        assert re.findall(r'^REPETITION (\d+)$', text, re.MULTILINE) == list(map(str, range(20)))
        assert text.count('3 passed; 0 failed') == 20 and 'test result: FAILED' not in text
        for name, count in expected_counts.items():
            assert list(map(int, re.findall(name + r'=(\d+)', text))) == [count] * 20
        for row in rows:
            assert row['exit_code'] == 0 and row['counts'] == expected_counts
            total += sum(row['counts'].values())
    for name, expected_failures in [('negative-commit-ack', 2), ('negative-inbox-ack', 1), ('negative-rollback', 2)]:
        text = log(name)
        assert 'test result: FAILED' in text and f'{expected_failures} failed' in text
        if name == 'negative-rollback':
            assert 'connection.is_autocommit()' in text
        else:
            assert 'left: Ok(' in text and 'right: Err(Conflict)' in text
    for name in ['stable-workspace', 'msrv-sqlite']:
        text = log(name)
        assert 'test result: FAILED' not in text
        assert '39 passed; 0 failed' in text and '72 passed; 0 failed' in text and '45 passed; 0 failed' in text
    qualification = json.loads((HERE / 'qualification.json').read_text())
    assert qualification and all(item['exit_code'] == 0 for item in qualification.values())
    for name, item in qualification.items():
        assert (HERE / item['log']).is_file(), name
    assert total == 1120
    print('Verified 1,120 deferred COMMIT-failure cases, three negative controls, final qualification, and test-only source changes.')


if __name__ == '__main__':
    main()
