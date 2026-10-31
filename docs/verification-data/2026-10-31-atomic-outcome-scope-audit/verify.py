"""Read-only evidence verification. Run from any directory; no benchmarks or builds."""
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


def main():
    manifest = json.loads((HERE / 'manifest.json').read_text())
    actual = {str(p.relative_to(HERE)): digest(p.read_bytes())
              for p in HERE.rglob('*') if p.is_file() and p.name != 'manifest.json'}
    assert actual == manifest, 'archive file set or content differs'
    for name, expected in json.loads((HERE / 'source-hashes.json').read_text()).items():
        assert digest((ROOT / name).read_bytes()) == expected, name
    production = 'crates/penelope-ports/src/implementation.rs'
    count = 0
    for label in ['borrowed-all', 'outlined-scope', 'action-stage', 'single-specialization']:
        folder = HERE / label
        metadata = json.loads((folder / 'metadata.json').read_text())
        baseline = subprocess.check_output(['git', 'show', metadata['source_parent'] + ':' + production], cwd=ROOT)
        assert digest(baseline) == metadata['sources']['baseline'][production]
        assert (ROOT / production).read_bytes() == baseline, 'rejected runtime change retained'
        for name, expected in metadata['harness_sha256'].items():
            assert digest((ROOT / name).read_bytes()) == expected, name
        with tempfile.TemporaryDirectory(prefix='penelope-scope-audit-') as temp:
            work = Path(temp)
            source = work / production
            source.parent.mkdir(parents=True)
            source.write_bytes(baseline)
            patch = gzip.decompress((folder / 'runtime.patch.gz').read_bytes())
            subprocess.run(['git', 'apply', '-'], input=patch, cwd=work, check=True)
            assert digest(source.read_bytes()) == metadata['sources']['candidate'][production]
            for name in ['raw.jsonl.gz', 'allocations.jsonl.gz', 'measurement.json']:
                shutil.copyfile(folder / name, work / name)
            result = subprocess.run([sys.executable, str(folder / 'analyze.py'), str(work)], stdout=subprocess.DEVNULL)
            assert result.returncode == 0, label
            assert (work / 'summary.json').read_bytes() == (folder / 'summary.json').read_bytes(), label
        for name in ['baseline-tests.txt.gz', 'candidate-tests.txt.gz']:
            log = gzip.decompress((folder / name).read_bytes()).decode()
            assert 'multi_outcome_cases=1536' in log and 'test result: FAILED' not in log
        negative = gzip.decompress((folder / 'negative-control.txt.gz').read_bytes()).decode()
        assert 'atomic scope validation allocated: long=true' in negative
        summary = json.loads((folder / 'summary.json').read_text())
        assert any(r['ci_low'] > 0 for r in summary['latency']), 'rejection evidence missing'
        count += summary['timed_validations']
    negative = gzip.decompress((HERE / 'scope-negative-control.txt.gz').read_bytes()).decode()
    assert 'length=5 rank=0 mask=1 competing=0' in negative
    assert 'OutcomeScopeMismatch' in negative and 'test result: FAILED' in negative
    for name in ['final-ports', 'final-stable', 'final-msrv']:
        log = gzip.decompress((HERE / (name + '.txt.gz')).read_bytes()).decode()
        assert 'test result: FAILED' not in log and 'allocations=4 reallocations=0 requested_bytes=512' in log
    qualification = json.loads((HERE / 'qualification.json').read_text())
    assert qualification and all(entry['exit_code'] == 0 for entry in qualification.values())
    assert count == 90017664
    print('Verified four rejected runtime candidates, 256 latency summaries, 128 allocation comparisons, and 90,017,664 timed validations.')


if __name__ == '__main__':
    main()
