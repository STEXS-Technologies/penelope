"""Read-only verification for the pinned retirement COMMIT boundary audit."""
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
    actual = {str(p.relative_to(HERE)): digest(p.read_bytes()) for p in HERE.rglob('*') if p.is_file() and p.name != 'manifest.json'}
    assert actual == json.loads((HERE / 'manifest.json').read_text())
    for path, expected in json.loads((HERE / 'source-hashes.json').read_text()).items():
        assert digest((ROOT / path).read_bytes()) == expected, path
    metadata = json.loads((HERE / 'metadata.json').read_text())
    prefix = 'crates/penelope-sqlite/src/'
    for name in ['lib.rs', 'tasks.rs', 'lifecycle.rs']:
        path = prefix + name
        prior = subprocess.check_output(['git', 'show', metadata['parent'] + ':' + path], cwd=ROOT)
        assert (ROOT / path).read_bytes() == prior, 'release source changed: ' + name
    path = prefix + 'task_commit_boundary_tests.rs'
    prior = subprocess.check_output(['git', 'show', metadata['parent'] + ':' + path], cwd=ROOT)
    assert (ROOT / path).read_bytes().startswith(prior), 'previous test code changed'
    total = 0
    for toolchain in ['stable', 'msrv']:
        text = log(toolchain + '-repetitions')
        rows = json.loads((HERE / (toolchain + '-repetitions.json')).read_text())['repetitions']
        assert [row['index'] for row in rows] == list(range(20))
        assert re.findall(r'^REPETITION (\d+)$', text, re.MULTILINE) == list(map(str, range(20)))
        assert text.count('1 passed; 0 failed') == 20 and 'test result: FAILED' not in text
        assert re.findall(r'deferred_retirement_cases=(\d+)', text) == ['12'] * 20
        for row in rows:
            assert row['exit_code'] == 0 and row['counts'] == {'deferred_retirement_cases': 12}
            total += sum(row['counts'].values())
    for name, marker in {
        'negative-ack': 'left: Ok(',
        'negative-rollback': 'journal.connection.is_autocommit()',
        'negative-scope': 'all target history must be deleted inside the failing transaction',
    }.items():
        text = log(name)
        assert 'test result: FAILED' in text and '1 failed' in text and marker in text
    for name in ['stable-workspace', 'msrv-sqlite']:
        text = log(name)
        assert 'test result: FAILED' not in text
        assert all(f'{count} passed; 0 failed' in text for count in [42, 72, 45])
    qualification = json.loads((HERE / 'qualification.json').read_text())
    assert qualification and all(item['exit_code'] == 0 for item in qualification.values())
    for name, item in qualification.items():
        assert (HERE / item['log']).is_file(), name
    assert total == 480
    print('Verified 480 retirement COMMIT-failure cases, three negative controls, final qualification, and unchanged release sources.')


if __name__ == '__main__':
    main()
