"""Read-only verification of borrowed timer claim validation evidence."""
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
def digest(data):
    return hashlib.sha256(data).hexdigest()
def log(name):
    return gzip.decompress((HERE / (name + '.txt.gz')).read_bytes()).decode()

def main():
    actual = {str(p.relative_to(HERE)): digest(p.read_bytes()) for p in HERE.rglob('*') if p.is_file() and p.name != 'manifest.json'}
    assert actual == json.loads((HERE / 'manifest.json').read_text())
    for name, expected in json.loads((HERE / 'source-hashes.json').read_text()).items():
        assert digest((ROOT / name).read_bytes()) == expected, name
    metadata = json.loads((HERE / 'metadata.json').read_text())
    for name, expected in metadata['harness_sha256'].items():
        assert digest((ROOT / name).read_bytes()) == expected, name
    original = subprocess.check_output(['git', 'show', metadata['parent'] + ':' + metadata['source']], cwd=ROOT)
    assert digest(original) == metadata['sources']['baseline']
    assert digest((ROOT / metadata['source']).read_bytes()) == metadata['sources']['candidate']
    with tempfile.TemporaryDirectory() as temporary:
        scratch = Path(temporary)
        source = scratch / metadata['source']
        source.parent.mkdir(parents=True)
        source.write_bytes(original)
        subprocess.run(['git', 'apply', '--unsafe-paths', '-'], input=gzip.decompress((HERE / 'runtime.patch.gz').read_bytes()), cwd=scratch, check=True)
        assert digest(source.read_bytes()) == metadata['sources']['candidate']
        for name in ['raw.jsonl.gz', 'allocations.jsonl.gz', 'measurement.json']:
            shutil.copyfile(HERE / name, scratch / name)
        subprocess.run([sys.executable, str(HERE / 'analyze.py'), str(scratch)], check=True, stdout=subprocess.DEVNULL)
        assert (scratch / 'summary.json').read_bytes() == (HERE / 'summary.json').read_bytes()
    summary = json.loads((HERE / 'summary.json').read_text())
    assert summary['timed_validations'] == 153600000
    assert len(summary['latency']) == 64 and not any(row['ci_low'] > 0 for row in summary['latency'])
    valid = [row for row in summary['latency'] if row['profile'].endswith('-valid')]
    assert len(valid) == 8 and all(row['ci_high'] < 0 for row in valid)
    for variant in ['baseline', 'candidate']:
        text = log(variant + '-tests')
        assert 'timer_legacy_cases=18432 profiles=32' in text and 'test result: ok.' in text
    assert '23-valid' in log('baseline-guard') and 'left: (4, 0, 92)' in log('baseline-guard')
    assert 'timer_claim_profiles=32 allocations=0 reallocations=0 requested_bytes=0' in log('candidate-guard')
    for name in ['stable-workspace', 'msrv']:
        text = log(name)
        assert 'test result: ok.' in text and 'test result: FAILED' not in text and 'error:' not in text
        assert 'timer_legacy_cases=' not in text or 'timer_legacy_cases=18432' in text
        assert 'timer_claim_profiles=32 allocations=0' in text
        assert all(f'{count} passed; 0 failed' in text for count in [44, 72, 45])
    for name in ['clippy', 'msrv-check']:
        assert 'Finished `dev`' in log(name) and 'error:' not in log(name)
    assert not log('fmt').strip() and not log('layers').strip()
    qualification = json.loads((HERE / 'qualification.json').read_text())
    assert qualification and all(row['exit_code'] == 0 for row in qualification.values())
    print('Verified 153,600,000 validations, 128 allocation observations, legacy parity, zero-allocation guard and qualification.')

if __name__ == '__main__':
    main()
