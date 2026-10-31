"""Check all three archived experiments without changing repository source."""
from pathlib import Path
import gzip
import hashlib
import json
import subprocess
import sys
import tempfile

archive = Path(__file__).resolve().parent
root = archive.parents[2]


def sha(data):
    return hashlib.sha256(data).hexdigest()


def log(directory, name):
    return gzip.decompress((directory / (name + '.gz')).read_bytes()).decode()


for name, expected in json.loads((archive / 'manifest.json').read_text()).items():
    assert sha((archive / name).read_bytes()) == expected, name
for name, expected in json.loads((archive / 'source-hashes.json').read_text()).items():
    assert sha((root / name).read_bytes()) == expected, name
for directory in [archive, archive / 'rejected-option', archive / 'rejected-shared-validation']:
    metadata = json.loads((directory / 'build-metadata.json').read_text())
    for name, expected in metadata['harness_sha256'].items():
        assert sha((root / name).read_bytes()) == expected, name
    with tempfile.TemporaryDirectory(prefix='penelope-single-append-verify-') as temporary:
        output = Path(temporary)
        for name, expected in metadata['sources']['baseline'].items():
            data = subprocess.check_output(['git', 'show', metadata['source_parent'] + ':' + name], cwd=root)
            assert sha(data) == expected
            destination = output / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.write_bytes(data)
        patch = output / 'candidate.patch'
        patch.write_bytes(gzip.decompress((directory / 'runtime.patch.gz').read_bytes()))
        subprocess.run(['git', 'apply', str(patch)], cwd=output, check=True)
        for name, expected in metadata['sources']['candidate'].items():
            assert sha((output / name).read_bytes()) == expected
            if directory == archive:
                assert sha((root / name).read_bytes()) == expected
        for name in ['raw.jsonl.gz', 'allocations.jsonl.gz', 'measurement.json']:
            (output / name).write_bytes((directory / name).read_bytes())
        subprocess.run([sys.executable, str(directory / 'analyze.py'), str(output)], check=True, stdout=subprocess.DEVNULL)
        assert (output / 'summary.json').read_bytes() == (directory / 'summary.json').read_bytes()
    for label in ['baseline', 'candidate']:
        text = log(directory, label + '-tests.txt')
        assert 'test result: FAILED' not in text
        assert 'single_and_batch_appends_share_duplicate_tracking_through_the_replay_bound ... ok' in text
    negative = log(directory, 'allocation-negative-control.txt')
    assert 'single append allocated a temporary set' in negative
    assert 'left: 1' in negative and 'right: 0' in negative
for name in ['stable.txt', 'msrv.txt']:
    text = log(archive, name)
    assert 'test result: FAILED' not in text
    assert 'single_append_allocations=0 verified=true' in text
    assert text.count('scope_rejection_allocations=1 repaired=true') == 3
    assert text.count('scope_rejection_allocations=0 repaired=true') == 1
    assert 'single_and_batch_appends_share_duplicate_tracking_through_the_replay_bound ... ok' in text
    assert 'every_scope_component_is_checked_before_sequence_without_mutating_the_log ... ok' in text
assert 'Finished' in log(archive, 'clippy.txt')
assert log(archive, 'static-checks.txt').count('exit status: 0') == 2
assert 'Code graph updated' in log(archive, 'graphify.txt')
print('Verified current sources, three reconstructed candidate patches, hashes, qualification and negative controls; recomputed 96 latency and 48 allocation comparisons from 2,304 timing rows and 768 allocation samples.')
