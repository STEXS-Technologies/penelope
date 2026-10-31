"""Verify archived evidence and source hashes; recompute in a temporary directory."""
from pathlib import Path
import gzip
import hashlib
import json
import subprocess
import sys
import tempfile

archive = Path(__file__).resolve().parent
root = archive.parents[2]


def digest(data):
    return hashlib.sha256(data).hexdigest()


def log(name):
    return gzip.decompress((archive / (name + '.gz')).read_bytes()).decode()


for name, expected in json.loads((archive / 'manifest.json').read_text()).items():
    assert digest((archive / name).read_bytes()) == expected, name
for name, expected in json.loads((archive / 'source-hashes.json').read_text()).items():
    assert digest((root / name).read_bytes()) == expected, name
metadata = json.loads((archive / 'build-metadata.json').read_text())
for name, expected in metadata['sources']['baseline'].items():
    assert digest(subprocess.check_output(['git', 'show', metadata['source_parent'] + ':' + name], cwd=root)) == expected
for name, expected in metadata['sources']['candidate'].items():
    assert digest((root / name).read_bytes()) == expected
for name, expected in metadata['harness_sha256'].items():
    assert digest((root / name).read_bytes()) == expected
for label in ['baseline', 'candidate']:
    text = log(label + '-tests.txt')
    assert 'every_scope_component_is_checked_before_sequence_without_mutating_the_log ... ok' in text
    assert 'test result: FAILED' not in text
assert 'scope rejection allocated 5 times' in log('allocation-negative-control.txt')
for name in ['stable.txt', 'msrv.txt']:
    text = log(name)
    assert 'test result: FAILED' not in text
    assert text.count('scope_rejection_allocations=1 repaired=true') == 4
    assert 'every_scope_component_is_checked_before_sequence_without_mutating_the_log ... ok' in text
assert 'Finished' in log('clippy.txt')
assert log('static-checks.txt').count('exit status: 0') == 2
assert 'Code graph updated' in log('graphify.txt')
with tempfile.TemporaryDirectory(prefix='penelope-outcome-log-verify-') as directory:
    output = Path(directory)
    for name in ['raw.jsonl.gz', 'allocations.jsonl.gz', 'measurement.json']:
        (output / name).write_bytes((archive / name).read_bytes())
    subprocess.run([sys.executable, str(archive / 'analyze.py'), directory], check=True, stdout=subprocess.DEVNULL)
    assert (output / 'summary.json').read_bytes() == (archive / 'summary.json').read_bytes()
print('Verified source/archive hashes, qualification and negative control; recomputed 32 latency and 16 allocation comparisons (768 timing rows, 256 allocation samples).')
