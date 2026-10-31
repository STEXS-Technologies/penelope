"""Read-only verification of the pinned timer COMMIT boundary audit."""
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
    for path in metadata['unchanged_release_sources']:
        original = subprocess.check_output(['git', 'show', metadata['parent'] + ':' + path], cwd=ROOT)
        assert (ROOT / path).read_bytes() == original, path
    path = 'crates/penelope-sqlite/src/process_commit_boundary_tests.rs'
    original = subprocess.check_output(['git', 'show', metadata['parent'] + ':' + path], cwd=ROOT)
    assert (ROOT / path).read_bytes().startswith(original), 'previous tests changed'
    total = 0
    for toolchain in ['stable', 'msrv']:
        text = log(toolchain + '-repetitions')
        rows = json.loads((HERE / (toolchain + '-repetitions.json')).read_text())
        assert [row['index'] for row in rows] == list(range(20))
        assert re.findall(r'^REPETITION (\d+)$', text, re.MULTILINE) == list(map(str, range(20)))
        assert text.count('1 passed; 0 failed') == 20 and 'test result: FAILED' not in text
        assert re.findall(r'deferred_timer_cases=(\d+)', text) == ['16'] * 20
        for row in rows:
            assert row['exit_code'] == 0 and row['cases'] == 16
            total += row['cases']
    assert total == 640
    negative = json.loads((HERE / 'negative-results.json').read_text())
    assert set(negative) == {'schedule', 'cancel', 'claim_due', 'acknowledge', 'rollback', 'quota'}
    for name, code in negative.items():
        text = log('negative-' + name)
        assert code != 0 and 'test result: FAILED' in text and '1 failed' in text
        if name == 'rollback':
            assert 'is_autocommit()' in text and 'assertion failed:' in text
        elif name == 'quota':
            assert 'quota must charge exact timer bytes once' in text
        else:
            assert 'left: Ok(' in text
    for name in ['stable-workspace', 'msrv-sqlite']:
        text = log(name)
        assert 'test result: FAILED' not in text and 'error:' not in text
        assert all(f'{count} passed; 0 failed' in text for count in [44, 72, 45])
    assert 'Finished `dev`' in log('clippy') and 'error:' not in log('clippy')
    assert not log('fmt').strip() and not log('layers').strip()
    qualification = json.loads((HERE / 'qualification.json').read_text())
    assert qualification and all(row['exit_code'] == 0 for row in qualification.values())
    for row in qualification.values():
        assert (HERE / row['log']).is_file()
    print('Verified 640 timer COMMIT-failure cases, six negative controls, unchanged release sources and qualification.')

if __name__ == '__main__':
    main()
