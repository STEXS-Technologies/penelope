from pathlib import Path
import contextlib
import hashlib
import importlib.util
import io
import json
root=Path(__file__).resolve().parents[3]
evidence=Path(__file__).resolve().parent
script=root/'scripts/benchmark-application-admission.py'
spec=importlib.util.spec_from_file_location('admission',script)
module=importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
metadata=json.loads((evidence/'metadata.json').read_text())
original=(evidence/'summary.json').read_bytes()
with contextlib.redirect_stdout(io.StringIO()):summary=module.summarize(evidence)
assert (evidence/'summary.json').read_bytes()==original
assert len(summary)==4 and all(row['block_bootstrap_95_percent'][1]<0 for row in summary)
rows=[json.loads(line) for line in (evidence/'samples.jsonl').read_text().splitlines()]
assert len(rows)==192 and sum(row['workflows'] for row in rows)==768
for mode in ['preaccepted','atomic']:
    assert sum(row['workflows'] for row in rows if row['admission']==mode)==384
for path,digest in metadata['source_sha256'].items():
    assert hashlib.sha256((root/path).read_bytes()).hexdigest()==digest,path
for label in ['stable','msrv']:
    text=(evidence/f'{label}.txt').read_text()
    assert 'test result: FAILED' not in text
    for name in ['all_workflow_paths_match_identical_business_work','admission_modes_preserve_identical_completed_rows','failed_atomic_admission_rolls_back_input_while_preacceptance_remains_durable','recovery_rejects_missing_business_result_evidence']:
        assert f'{name} ... ok' in text,name
    assert text.count('"verified":true')==12
assert 'Finished' in (evidence/'clippy.txt').read_text()
assert 'Ran 6 tests' in (evidence/'python-tests.txt').read_text()
assert 'Verified 24 paired samples' in (evidence/'runner-smoke.txt').read_text()
print('Verified 192 paired rows / 768 workflows, both admissions, exact sources, recomputed intervals and qualification.')
