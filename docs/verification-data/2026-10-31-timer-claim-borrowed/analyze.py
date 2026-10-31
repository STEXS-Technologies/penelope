from pathlib import Path
import collections,gzip,json,math,random,statistics,sys
out=Path(sys.argv[1]).resolve() if len(sys.argv)>1 else Path(__file__).resolve().parent
def read(name):
 p=out/name
 return p.read_text() if p.exists() else gzip.decompress((out/(name+'.gz')).read_bytes()).decode()
cases=['valid','first-scope','last-scope','owner','not-due','expired','bad-kind','oversized-limit']
profiles={f'{length}-{case}' for length in [5,22,23,128] for case in cases}
expected=lambda profile: 'Ok(())' if profile.split('-',1)[1]=='valid' else 'Err(TimedOut)' if profile.endswith('-expired') else 'Err(QuotaExceeded)' if profile.endswith('-oversized-limit') else 'Err(Invariant)'
rows=[json.loads(line) for line in read('raw.jsonl').splitlines()];assert len(rows)==1536
keys=set();groups=collections.defaultdict(list)
for row in rows:
 assert row['profile'] in profiles and row['campaign'] in range(2) and row['block'] in range(6) and row['slot'] in range(4)
 order=['baseline','candidate','candidate','baseline'] if row['block']%2==0 else ['candidate','baseline','baseline','candidate']
 assert row['variant']==order[row['slot']] and row['iterations']==100000 and row['elapsed_ns']>0
 assert row['result']==expected(row['profile'])
 key=tuple(row[k] for k in ['campaign','block','slot','profile']);assert key not in keys;keys.add(key)
 groups[(row['campaign'],row['profile'])].append(row)
latency=[]
for key,data in sorted(groups.items()):
 blocks=[]
 for block in range(6):
  values={label:[r['elapsed_ns']/r['iterations'] for r in data if r['block']==block and r['variant']==label] for label in ['baseline','candidate']}
  assert all(len(values[label])==2 for label in values)
  blocks.append(statistics.mean(map(math.log,values['candidate']))-statistics.mean(map(math.log,values['baseline'])))
 rng=random.Random(20261031);boot=sorted(math.expm1(statistics.mean(rng.choices(blocks,k=6)))*100 for _ in range(20000))
 latency.append(dict(zip(['campaign','profile'],key))|{'baseline_ns':statistics.median(r['elapsed_ns']/r['iterations'] for r in data if r['variant']=='baseline'),'candidate_ns':statistics.median(r['elapsed_ns']/r['iterations'] for r in data if r['variant']=='candidate'),'change_pct':math.expm1(statistics.mean(blocks))*100,'ci_low':boot[500],'ci_high':boot[19499]})
assert len(latency)==64
alloc=[json.loads(line) for line in read('allocations.jsonl').splitlines()];assert len(alloc)==128
keys=set();allocation=[]
for row in alloc:
 assert row['profile'] in profiles and row['variant'] in ['baseline','candidate'] and row['repeat'] in range(2)
 assert row['result']==expected(row['profile'])
 key=tuple(row[k] for k in ['repeat','variant','profile']);assert key not in keys;keys.add(key)
 length=int(row['profile'].split('-',1)[0]);case=row['profile'].split('-',1)[1]
 allocations=4 if row['variant']=='baseline' and length>22 and case not in ['bad-kind','oversized-limit'] else 0
 assert (row['allocations'],row['reallocations'],row['requested_bytes'])==(allocations,0,allocations*length)
for profile in sorted(profiles):
 result={'profile':profile}
 for variant in ['baseline','candidate']:
  data=[r for r in alloc if r['profile']==profile and r['variant']==variant];assert len(data)==2
  result[variant]={k:data[0][k] for k in ['allocations','reallocations','requested_bytes']}
 allocation.append(result)
assert sum(r['iterations'] for r in rows)==153600000
measurement=json.loads(read('measurement.json'));assert (measurement['ordinary_processes'],measurement['allocation_processes'])==(48,4)
(out/'summary.json').write_text(json.dumps({'latency':latency,'allocation':allocation,'timed_validations':153600000},indent=2)+'\n')
for row in latency:print(row)
