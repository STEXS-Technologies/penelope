"""Read-only evidence verification; run from this change's checkout."""
from pathlib import Path
import gzip
import hashlib
import json
import shutil
import subprocess
import sys
import tempfile

archive = Path(__file__).resolve().parent
repo = archive.parents[2]
def logtext(path):
    return gzip.decompress(path.with_name(path.name + '.gz').read_bytes()).decode()

def digest(data):
    return hashlib.sha256(data).hexdigest()

manifest = json.loads((archive / 'manifest.json').read_text())
assert set(manifest) == {str(p.relative_to(archive)) for p in archive.rglob('*') if p.is_file() and p.name != 'manifest.json'}
for name, expected in manifest.items():
    assert digest((archive / name).read_bytes()) == expected, name
for name, expected in json.loads((archive / 'source-hashes.json').read_text()).items():
    assert digest((repo / name).read_bytes()) == expected, name

for label in ['initial', 'qualified']:
    folder = archive / label
    metadata = json.loads((folder / 'metadata.json').read_text())
    baseline = subprocess.check_output(['git', 'show', metadata['parent'] + ':' + metadata['source']], cwd=repo)
    assert digest(baseline) == metadata['sources']['baseline']
    assert digest((repo / metadata['source']).read_bytes()) == metadata['sources']['candidate']
    for name, expected in metadata['harness_sha256'].items():
        data = gzip.decompress((folder / 'encoding-harness.rs.gz').read_bytes()) if label == 'initial' and name.endswith('/effect_key_encoding.rs') else (repo / name).read_bytes()
        assert digest(data) == expected, (label, name)
    with tempfile.TemporaryDirectory() as temporary:
        scratch = Path(temporary)
        source = scratch / metadata['source']
        source.parent.mkdir(parents=True)
        source.write_bytes(baseline)
        subprocess.run(['git', 'apply', '--unsafe-paths', '-'], input=gzip.decompress((archive / 'runtime.patch.gz').read_bytes()), cwd=scratch, check=True)
        assert digest(source.read_bytes()) == metadata['sources']['candidate']
        for name in ['raw.jsonl.gz', 'allocations.jsonl.gz', 'public-allocations.jsonl.gz', 'measurement.json']:
            shutil.copyfile(folder / name, scratch / name)
        subprocess.run([sys.executable, str(folder / 'analyze.py'), str(scratch)], check=True, stdout=subprocess.DEVNULL)
        assert (scratch / 'summary.json').read_bytes() == (folder / 'summary.json').read_bytes()
    summary = json.loads((folder / 'summary.json').read_text())
    assert len(summary['latency']) == 40 and all(r['ci_high'] < 0 for r in summary['latency'])
    assert summary['timed_encodings'] == 96000000
    for variant in ['baseline', 'candidate']:
        log = logtext(folder / (variant + '-tests.txt'))
        assert 'effect_key_legacy_cases=10005' in log and 'test result: ok.' in log
    assert 'short kind=0' in logtext(folder / 'baseline-guard.txt')
    assert 'left: 5' in logtext(folder / 'baseline-guard.txt')
    assert 'effect_key_profiles=20 allocations=1 exact_capacity=true' in logtext(folder / 'candidate-guard.txt')
for name in ['stable-workspace.txt', 'msrv.txt']:
    log = logtext(archive / name)
    assert 'test result: ok.' in log and 'test result: FAILED' not in log and 'error:' not in log
for name in ['clippy.txt', 'msrv-check.txt']:
    log = logtext(archive / name)
    assert 'Finished `dev`' in log and 'error:' not in log
assert not logtext(archive / 'fmt.txt').strip()
assert not logtext(archive / 'layers.txt').strip()
print('Verified both experiments, byte parity, allocation guards, source identity and qualification.')
