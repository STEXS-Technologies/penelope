"""Verify hashes, failure reproduction, repeated rejection and allocation evidence."""
from pathlib import Path
import gzip, hashlib, json
out = Path(__file__).resolve().parent
meta = json.loads((out / 'metadata.json').read_text())
for name, digest in meta['source_sha256'].items():
    assert hashlib.sha256(Path(name).read_bytes()).hexdigest() == digest, name
for name, digest in meta['archive_sha256'].items():
    assert hashlib.sha256((out / name).read_bytes()).hexdigest() == digest, name
baseline = gzip.decompress((out / 'baseline.log.gz').read_bytes()).decode()
assert baseline.count('accepted=true unchanged=false') == 4
assert baseline.count('result=Err(Conflict)') == 2
repeat = gzip.decompress((out / 'repeat.log.gz').read_bytes()).decode()
assert repeat.count('accepted=false unchanged=true') == 360
assert repeat.count('test result: ok. 1 passed; 0 failed') == 20
traffic = {}
for variant in ['baseline', 'candidate']:
    rows = [json.loads(line) for line in (out / (variant + '-allocations.jsonl')).read_text().splitlines()]
    assert len(rows) == 48
    traffic[variant] = {(r['profile'], r['allocations'], r['reallocations'], r['requested_bytes']) for r in rows}
assert traffic['baseline'] == traffic['candidate'] == {(p, 1, 0, 192) for p in ['record', 'padded', 'end']}
for label in ['workspace', 'msrv', 'fixed']:
    text = gzip.decompress((out / (label + '.log.gz')).read_bytes()).decode()
    assert 'test result: FAILED' not in text, label
    assert 'test result: ok.' in text, label
print('Verified hashes, four baseline false successes, 360 final rejections, repair/reopen runs and unchanged read allocation counts.')
