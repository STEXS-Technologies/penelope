"""Verify both campaigns, exact traffic, malformed identities and final qualification."""
from pathlib import Path
import gzip, hashlib, json, subprocess, tempfile
out = Path(__file__).resolve().parent
meta = json.loads((out / 'final-campaign.json').read_text())
for name, digest in meta['source_sha256'].items():
    assert hashlib.sha256(Path(name).read_bytes()).hexdigest() == digest, name
for name, digest in json.loads((out / 'archive-hashes.json').read_text()).items():
    assert hashlib.sha256((out / name).read_bytes()).hexdigest() == digest, name
for script, raw, result in [('analyze.py', 'initial-runs.jsonl', 'summary.json'), ('analyze-three.py', 'final-runs.jsonl', 'final-summary.json')]:
    with tempfile.TemporaryDirectory() as directory:
        work = Path(directory)
        destination = 'runs.jsonl' if script == 'analyze.py' else raw
        (work / destination).write_bytes(gzip.decompress((out / (raw + '.gz')).read_bytes()))
        (work / script).write_bytes((out / script).read_bytes())
        subprocess.run(['python3', str(work / script)], check=True, capture_output=True)
        expected = 'initial-summary.json' if script == 'analyze.py' else result
        assert json.loads((work / result).read_text()) == json.loads((out / expected).read_text())
initial = json.loads((out / 'initial-binary-hashes.json').read_text())
final = json.loads((out / 'final-binary-hashes.json').read_text())
for mode in ['latency', 'allocations']:
    assert initial['baseline-' + mode] == final['baseline-' + mode]
    assert initial['candidate-' + mode] == final['uncached-' + mode]
baseline = gzip.decompress((out / 'baseline.log.gz').read_bytes()).decode()
assert baseline.count('expected=Err(Corrupt) actual=Err(Conflict)') == 4
repeat = gzip.decompress((out / 'repeat.log.gz').read_bytes()).decode()
assert repeat.count('test result: ok. 2 passed; 0 failed') == 20
assert repeat.count(' reserve=') == 540
for label in ['final-workspace', 'final-msrv', 'final-tests', 'final-conformance']:
    text = gzip.decompress((out / (label + '.log.gz')).read_bytes()).decode()
    assert 'test result: FAILED' not in text, label
    assert 'test result: ok.' in text, label
conformance = gzip.decompress((out / 'final-conformance.log.gz').read_bytes()).decode()
assert 'ae3d94bc22c7835eaa3fdcd766e3c117929c12bb' in conformance
assert 'test result: ok. 7 passed; 0 failed' in conformance
print('Verified both campaigns, all 12 statistics, exact allocation traffic, reused binary hashes, four baseline classification differences, 540 final cases and seven conformance tests.')
