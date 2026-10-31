"""Verify the archived reference campaign against its exact source and checks."""
from pathlib import Path
import hashlib
import json
import sys
root = Path(__file__).resolve().parents[3]
evidence = Path(__file__).resolve().parent
sys.path.insert(0,str(root/'scripts'))
from application_workloads_summary import summarize
metadata = json.loads((evidence/'metadata.json').read_text())
rows = [json.loads(line) for line in (evidence/'samples.jsonl').read_text().splitlines()]
assert len(rows) == 1152
assert sum(row['workflows'] for row in rows) == 11520
assert summarize(rows,metadata) == json.loads((evidence/'summary.json').read_text())
for path, digest in metadata['source_sha256'].items():
    assert hashlib.sha256((root/path).read_bytes()).hexdigest() == digest,path
for toolchain in ['stable','msrv']:
    text = (evidence/f'{toolchain}.txt').read_text()
    assert 'test result: FAILED' not in text
    assert 'all_workflow_paths_match_identical_business_work ... ok' in text
    assert 'recovery_rejects_missing_business_result_evidence ... ok' in text
    assert 'module_entry_files_contain_only_declarations_and_exports ... ok' in text
    assert text.count('"verified":true') == 12
assert 'Finished' in (evidence/'clippy.txt').read_text()
assert 'Ran 3 tests' in (evidence/'python-tests.txt').read_text()
print('Verified 1,152 pairs / 11,520 managed workflows, exact source hashes, recomputed summaries and both toolchain checks.')
