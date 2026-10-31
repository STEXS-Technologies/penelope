"""Read-only verification of the exact process-record-buffer audit checkout."""
from pathlib import Path
import gzip
import hashlib
import json
import shutil
import subprocess
import sys
import tempfile

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
PRODUCTION = 'crates/penelope-sqlite/src/process.rs'


def digest(data):
    return hashlib.sha256(data).hexdigest()


def log(name):
    return gzip.decompress((HERE / (name + '.txt.gz')).read_bytes()).decode()


def main():
    actual = {str(p.relative_to(HERE)): digest(p.read_bytes())
              for p in HERE.rglob('*') if p.is_file() and p.name != 'manifest.json'}
    assert json.loads((HERE / 'manifest.json').read_text()) == actual
    for path, expected in json.loads((HERE / 'source-hashes.json').read_text()).items():
        assert digest((ROOT / path).read_bytes()) == expected, path
    total = 0
    regressions = {}
    for label in ['rejected-commit-buffer', 'record-buffer', 'confirmation']:
        folder = HERE / label
        metadata = json.loads((folder / 'metadata.json').read_text())
        baseline = subprocess.check_output(['git', 'show', metadata['parent'] + ':' + PRODUCTION], cwd=ROOT)
        assert digest(baseline) == metadata['sources']['baseline']
        patch_folder = HERE / 'record-buffer' if label == 'confirmation' else folder
        for path, expected in json.loads((folder / 'harness-hashes.json').read_text()).items():
            data = (ROOT / path).read_bytes()
            if label == 'rejected-commit-buffer' and path.endswith('process_encoding_fixture.rs'):
                data = data.replace(b'#[allow(clippy::unwrap_used)]\n', b'')
            assert digest(data) == expected, path
        with tempfile.TemporaryDirectory(prefix='penelope-record-buffer-') as temp:
            work = Path(temp)
            source = work / PRODUCTION
            source.parent.mkdir(parents=True)
            source.write_bytes(baseline)
            patch = gzip.decompress((patch_folder / 'runtime.patch.gz').read_bytes())
            subprocess.run(['git', 'apply', '-'], input=patch, cwd=work, check=True)
            assert digest(source.read_bytes()) == metadata['sources']['candidate']
            if label != 'rejected-commit-buffer':
                assert source.read_bytes() == (ROOT / PRODUCTION).read_bytes()
            for name in ['raw.jsonl.gz', 'allocations.jsonl.gz', 'measurement.json']:
                shutil.copyfile(folder / name, work / name)
            subprocess.run([sys.executable, str(folder / 'analyze.py'), str(work)], stdout=subprocess.DEVNULL, check=True)
            assert (work / 'summary.json').read_bytes() == (folder / 'summary.json').read_bytes(), label
        summary = json.loads((folder / 'summary.json').read_text())
        total += summary['timed_commits']
        if label != 'confirmation':
            for name in ['baseline-tests.txt.gz', 'candidate-tests.txt.gz']:
                text = gzip.decompress((folder / name).read_bytes()).decode()
                assert 'encoding_exact_bytes_profiles=3' in text and 'encoding_late_insert_rollbacks=2' in text
                assert 'test result: FAILED' not in text
        if label != 'rejected-commit-buffer':
            for row in summary['latency']:
                key = (row['long'], row['outcomes'], row['actions'])
                regressions[key] = regressions.get(key, 0) + (row['ci_low'] > 0)
        else:
            failures = [r for r in summary['latency'] if not r['long'] and r['outcomes'] == 128 and r['actions'] == 0]
            assert len(failures) == 2 and all(r['ci_low'] > 0 for r in failures)
    assert max(regressions.values()) == 1
    retained = json.loads((HERE / 'record-buffer/summary.json').read_text())['allocation']
    confirmed = json.loads((HERE / 'confirmation/summary.json').read_text())['allocation']
    assert retained == confirmed
    for profile in retained:
        for variant in ['baseline', 'candidate']:
            for counts in profile[variant].values():
                assert counts['min'] == counts['median'] == counts['max']
    assert (HERE / 'record-buffer/metadata.json').read_bytes() == (HERE / 'confirmation/metadata.json').read_bytes()
    assert 'test result: FAILED' in log('negative-clear') and 'assertion' in log('negative-clear')
    assert 'fresh encoding allocation budget exceeded: long=false outcomes=1 actions=128' in log('negative-baseline-guard')
    for name in ['final-stable', 'final-msrv']:
        text = log(name)
        assert 'test result: FAILED' not in text and '72 passed; 0 failed' in text
        assert 'process_encoding_allocations' in text and '"guarded":true' in text
    qualification = json.loads((HERE / 'qualification.json').read_text())
    assert qualification and all(value['exit_code'] == 0 for value in qualification.values())
    for name, info in qualification.items():
        assert (HERE / info['log']).is_file(), name
    assert total == 9216
    print('Verified 9,216 timed commits, 768 allocation observations, both candidate patches, negative controls, and final source hashes.')


if __name__ == '__main__':
    main()
