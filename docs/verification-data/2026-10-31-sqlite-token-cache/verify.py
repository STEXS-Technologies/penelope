"""Verify all evidence hashes, source/binary pins, pairs and recomputed metrics."""
from pathlib import Path
import gzip,hashlib,json,random,re,statistics,math
root=Path(__file__).resolve().parent
manifest=json.loads((root/'sha256.json').read_text())
assert set(manifest)=={p.name for p in root.iterdir() if p.is_file() and p.name!='sha256.json'}
for name,digest in manifest.items():
 assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest,name
metadata=json.loads((root/'build.json').read_text())
for variant in ['baseline','candidate']:
 source=gzip.decompress((root/f'compiled-{variant}-leases.rs.gz').read_bytes())
 assert hashlib.sha256(source).hexdigest()==metadata['variants'][variant]['source_sha256']
assert hashlib.sha256(gzip.decompress((root/'benchmark.rs.gz').read_bytes())).hexdigest()==metadata['benchmark_sha256']
rows=json.loads((root/'results.json').read_text());summary=json.loads((root/'summary.json').read_text())
assert len(rows)==40
assert {(r['pair'],r['variant']) for r in rows}=={(p,v) for p in range(20) for v in ['baseline','candidate']}
assert len({r['cpu'] for r in rows})==1
for row in rows:
 assert row['binary_sha256']==metadata['variants'][row['variant']]['binary_sha256']
 assert not row['stderr']
 assert len(row['stdout'].splitlines())==4
for timers in ['false','true']:
 for batch in [1,16]:
  metric=f'timers={timers} batch={batch}'
  values={}
  for row in rows:
   line=next(s for s in row['stdout'].splitlines() if s.startswith(metric+' '))
   match=re.fullmatch(r'timers=(false|true) batch=(1|16) iterations=400 elapsed_ns=(\d+)',line)
   assert match
   values[row['pair'],row['variant']]=int(match[3])
  ratios=[values[i,'candidate']/values[i,'baseline'] for i in range(20)]
  rng=random.Random(42)
  boot=sorted(statistics.mean(rng.choices(ratios,k=20)) for _ in range(10000))
  change=100*(statistics.mean(ratios)-1);interval=[100*(boot[i]-1) for i in [250,9749]]
  assert math.isclose(change,summary[metric]['mean_paired_change_pct'],abs_tol=1e-12)
  assert all(math.isclose(a,b,abs_tol=1e-12) for a,b in zip(interval,summary[metric]['bootstrap95_pct']))
  print(metric,change,interval)
repeated=json.loads((root/'repeated.json').read_text())
assert len(repeated)==50
assert {(r['compiler'],r['repeat']) for r in repeated}=={(c,n) for c in ['stable','1.85.0'] for n in range(25)}
assert all('1 passed; 0 failed' in r['stdout'] and not r['stderr'] for r in repeated)
for compiler in ['stable','1.85.0']:
 assert len({r['binary_sha256'] for r in repeated if r['compiler']==compiler})==1
for name in ['workspace','msrv']:
 log=gzip.decompress((root/(name+'.log.gz')).read_bytes()).decode()
 for count in [21,34,26]: assert f'test result: ok. {count} passed;' in log
 assert 'test result: FAILED' not in log and 'error: test failed' not in log
print('Verified 40 paired benchmark processes and 50 isolated rollback tests')
