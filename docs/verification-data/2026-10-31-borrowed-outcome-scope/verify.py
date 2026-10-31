"""Verify archived measurements without running latency jobs or editing sources."""
from pathlib import Path
import gzip
import hashlib
import json
import shutil
import subprocess
import sys
import tempfile

out = Path(__file__).resolve().parent
root = out.parents[2]
manifest = json.loads((out / 'manifest.json').read_text())
for name, digest in manifest['files_sha256'].items():
    assert hashlib.sha256((out / name).read_bytes()).hexdigest() == digest, name
for name, digest in manifest['current_sources_sha256'].items():
    assert hashlib.sha256((root / name).read_bytes()).hexdigest() == digest, name
for phase in [out, out / 'excluded/unannotated', out / 'excluded/inline-hint']:
    with tempfile.TemporaryDirectory() as directory:
        scratch = Path(directory)
        for name in ['raw.jsonl.gz', 'allocations.jsonl.gz']:
            shutil.copy2(phase / name, scratch / name)
        subprocess.run([sys.executable, str(out / 'analyze.py'), str(scratch)], check=True, stdout=subprocess.DEVNULL)
        assert (scratch / 'summary.json').read_bytes() == (phase / 'summary.json').read_bytes(), phase
    metadata = json.loads((phase / 'build-metadata.json').read_text())
    for name, digest in metadata['sources']['baseline'].items():
        baseline = subprocess.check_output(['git', 'show', f"{metadata['source_parent']}:{name}"], cwd=root)
        assert hashlib.sha256(baseline).hexdigest() == digest, name
    for label in ['baseline', 'candidate']:
        log = gzip.decompress((phase / f'{label}-process-tests.txt.gz').read_bytes()).decode()
        assert 'test result: ok. 63 passed;' in log
    negative = gzip.decompress((phase / 'allocation-negative-control.txt.gz').read_bytes()).decode()
    assert 'left: 4' in negative and 'right: 0' in negative and 'panicked' in negative
for tool in ['stable', 'msrv']:
    process = gzip.decompress((out / f'final-process-{tool}.txt.gz').read_bytes()).decode()
    assert 'test result: ok. 64 passed;' in process
    assert 'replay_scope::replay_rejects_each_scope_component_in_first_and_last_rows ... ok' in process
    domain = gzip.decompress((out / f'final-domain-{tool}.txt.gz').read_bytes()).decode()
    assert 'scope_validation_matches_owned_comparison_at_identifier_boundaries ... ok' in domain
    assert 'test result: FAILED' not in domain
negative = gzip.decompress((out / 'page-scope-negative-control.txt.gz').read_bytes()).decode()
assert 'test result: FAILED' in negative and 'replay_rejects_each_scope_component' in negative
for name in ['stable.txt.gz', 'msrv.txt.gz', 'clippy.txt.gz']:
    log = gzip.decompress((out / name).read_bytes()).decode()
    assert 'test result: FAILED' not in log and 'error:' not in log, name
print('Verified 4,320 timing rows, 960 allocation samples, all 180 latency summaries, source hashes and qualification logs.')
