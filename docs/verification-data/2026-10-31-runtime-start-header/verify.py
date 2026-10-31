"""Verify retained source pins, allocation counts, and validation outputs."""
from pathlib import Path
import gzip,hashlib,json
root=Path(__file__).resolve().parent
manifest=json.loads((root/'sha256.json').read_text())
assert set(manifest)=={p.name for p in root.iterdir() if p.is_file() and p.name!='sha256.json'}
for name,digest in manifest.items():
 assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest,name
metadata=json.loads((root/'metadata.json').read_text())
rows=json.loads((root/'allocations.json').read_text())
assert len(rows)==40
assert {(r['variant'],r['compiler'],r['repeat']) for r in rows}=={(v,c,n) for v in ['baseline','candidate'] for c in ['stable','1.85.0'] for n in range(10)}
for compiler,before,after in [('stable',245,244),('1.85.0',246,245)]:
 for variant,expected in [('baseline',before),('candidate',after)]:
  selected=[r for r in rows if r['compiler']==compiler and r['variant']==variant]
  assert {r['dispatch_allocations'] for r in selected}=={expected}
  assert len({r['binary_sha256'] for r in selected})==1
  assert {r['source_sha256'] for r in selected}=={metadata['source_sha256'][variant]}
  assert all(not r['stderr'] and f'supervisor dispatch={expected}' in r['stdout'] for r in selected)
  assert all('repeated public fences=[16, 16, 16]' in r['stdout'] and 'committed checkpoint ACK=0' in r['stdout'] for r in selected)
 print(compiler,before,'->',after)
for name in ['workspace','msrv']:
 log=gzip.decompress((root/(name+'.log.gz')).read_bytes()).decode()
 for count in [29,66,21,34,26]: assert f'test result: ok. {count} passed;' in log
 assert 'fixed_start_header_matches_legacy_wire_for_varied_bindings_and_lengths ... ok' in log
 assert 'test result: FAILED' not in log and 'error: test failed' not in log
assert 'test result: ok. 1 passed;' in gzip.decompress((root/'wire-final.log.gz').read_bytes()).decode()
print('Verified 40 real dispatch allocation probes and stable/MSRV validation')
