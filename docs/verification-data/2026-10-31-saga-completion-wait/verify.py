"""Check delayed-worker failure reproduction, successful repeats and qualification logs."""
from pathlib import Path
import gzip, hashlib, json, re
out = Path(__file__).resolve().parent
meta = json.loads((out / 'metadata.json').read_text())
for name, digest in meta['source_sha256'].items():
    assert hashlib.sha256(Path(name).read_bytes()).hexdigest() == digest, name
for name, digest in meta['archive_sha256'].items():
    assert hashlib.sha256((out / name).read_bytes()).hexdigest() == digest, name
baseline = gzip.decompress((out / 'baseline.log.gz').read_bytes()).decode()
assert 'left: Running' in baseline and 'right: Completed' in baseline
assert 'test result: FAILED' in baseline
repeat = gzip.decompress((out / 'repeat.log.gz').read_bytes()).decode()
assert repeat.count('test result: ok. 1 passed; 0 failed') == 20
samples = re.findall(r'delayed-worker elapsed_ms=(\d+) status=Completed', repeat)
assert len(samples) == 20 and all(int(s) >= 250 for s in samples)
for label in ['workspace', 'msrv', 'fixed', 'conformance']:
    text = gzip.decompress((out / (label + '.log.gz')).read_bytes()).decode()
    assert 'test result: FAILED' not in text, label
    assert 'test result: ok.' in text, label
conformance = gzip.decompress((out / 'conformance.log.gz').read_bytes()).decode()
assert meta['upstream_revision'] in conformance
assert 'test result: ok. 7 passed; 0 failed' in conformance
rerun = json.loads((out / 'prior-ci-rerun.json').read_text())
assert rerun['attempt'] == 2 and rerun['conclusion'] == 'success'
print('Verified source/archive hashes, old-window failure, 20 delayed-worker completions, seven signed conformance tests and successful same-commit CI rerun.')
