"""Verify retained evidence hashes, complete pairs, and reported intervals."""
from pathlib import Path
import gzip,hashlib,json,random,re,statistics
root=Path(__file__).resolve().parent
manifest=json.loads((root/'sha256.json').read_text())
for name,digest in manifest.items():
 assert hashlib.sha256((root/name).read_bytes()).hexdigest()==digest,name
rows=json.loads((root/'results.json').read_text())
assert len(rows)==40
assert {(r['pair'],r['variant']) for r in rows}=={(p,v) for p in range(20) for v in ['baseline','candidate']}
assert len({r['cpu'] for r in rows})==1
for timers in ['false','true']:
 values={}
 for row in rows:
  lines=row['stdout'].splitlines()
  assert len(lines)==2
  line=next(s for s in lines if s.startswith('timers='+timers+' '))
  match=re.fullmatch(r'timers=(false|true) iterations=2000 elapsed_ns=(\d+)',line)
  assert match
  values[row['pair'],row['variant']]=int(match[2])
 ratios=[values[i,'candidate']/values[i,'baseline'] for i in range(20)]
 rng=random.Random(42)
 boot=sorted(statistics.mean(rng.choices(ratios,k=20)) for _ in range(10000))
 print(timers,100*(statistics.mean(ratios)-1),[100*(boot[i]-1) for i in [250,9749]])
for name in ['workspace','msrv']:
 log=gzip.decompress((root/(name+'.log.gz')).read_bytes()).decode()
 assert 'test result: ok. 33 passed;' in log
 assert 'test result: ok. 26 passed;' in log
 assert 'test result: FAILED' not in log
 assert 'error: test failed' not in log
print('Evidence verified')
