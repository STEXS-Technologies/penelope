import gzip
import sys
from pathlib import Path
import json,math,random,statistics,collections
out=Path(sys.argv[1]).resolve() if len(sys.argv)>1 else Path(__file__).resolve().parent
def read(name):
 p=out/name
 return p.read_text() if p.exists() else gzip.decompress((out/(name+'.gz')).read_bytes()).decode()
rows=[json.loads(s) for s in read('raw.jsonl').splitlines()]
assert len(rows)==1440
keys=set()
groups=collections.defaultdict(list)
for r in rows:
 assert r['campaign'] in range(2) and r['block'] in range(6) and r['slot'] in range(4)
 assert r['scope'] in ['short','long'] and r['history'] in [1,128,4096]
 expected_requests={'first':(0,1),'last':(r['history']-1,1),'end':(r['history'],1),'past':(r['history']+17,1),'page':(0,128)}
 assert r['profile'] in expected_requests
 assert (r['from'],r['limit'])==expected_requests[r['profile']]
 key=tuple(r[k] for k in ['campaign','block','slot','scope','history','profile'])
 assert key not in keys;keys.add(key)
 assert r['iterations']==1000 and r['elapsed_ns']>0
 order=['baseline','candidate','candidate','baseline'] if r['block']%2==0 else ['candidate','baseline','baseline','candidate']
 assert r['variant']==order[r['slot']]
 groups[(r['campaign'],r['scope'],r['history'],r['profile'])].append(r)
summaries=[]
for key,data in sorted(groups.items()):
 blocks=[]
 for block in range(6):
  values={label:[r['elapsed_ns']/r['iterations'] for r in data if r['block']==block and r['variant']==label] for label in ['baseline','candidate']}
  assert all(len(v)==2 for v in values.values())
  blocks.append(statistics.mean(map(math.log,values['candidate']))-statistics.mean(map(math.log,values['baseline'])))
 rng=random.Random(20261031)
 boot=sorted(math.expm1(statistics.mean(rng.choices(blocks,k=6)))*100 for _ in range(20000))
 summaries.append(dict(zip(['campaign','scope','history','profile'],key))|{'baseline_ns':statistics.median(r['elapsed_ns']/r['iterations'] for r in data if r['variant']=='baseline'),'candidate_ns':statistics.median(r['elapsed_ns']/r['iterations'] for r in data if r['variant']=='candidate'),'change_pct':math.expm1(statistics.mean(blocks))*100,'ci_low':boot[500],'ci_high':boot[19499]})
assert len(summaries)==60
alloc=[json.loads(s) for s in read('allocations.jsonl').splitlines()];assert len(alloc)==320
allocation_summary=[]
for scope in ['short','long']:
 for profile in ['first','last','end','past','page']:
  comparison={'scope':scope,'profile':profile}
  for label in ['baseline','candidate']:
   subset=[r for r in alloc if r['scope']==scope and r['profile']==profile and r['variant']==label]
   assert sorted(r['sample'] for r in subset)==list(range(16))
   values={tuple(r[k] for k in ['allocations','reallocations','requested_bytes','rows']) for r in subset};assert len(values)==1,values
   comparison[label]=dict(zip(['allocations','reallocations','requested_bytes','rows'],values.pop()))
  assert comparison['baseline']['rows']==comparison['candidate']['rows']
  assert len({r['copied_bytes'] for r in alloc if r['scope']==scope and r['profile']==profile})==1
  comparison['removed_allocations']=comparison['baseline']['allocations']-comparison['candidate']['allocations']
  comparison['removed_requested_bytes']=comparison['baseline']['requested_bytes']-comparison['candidate']['requested_bytes']
  expected_rows=comparison['candidate']['rows']
  assert comparison['removed_allocations']==(4*expected_rows if scope=='long' else 0)
  assert comparison['removed_requested_bytes']==(512*expected_rows if scope=='long' else 0)
  allocation_summary.append(comparison)
(out/'summary.json').write_text(json.dumps({'latency':summaries,'allocation':allocation_summary},indent=2)+'\n')
for r in summaries:
 if r['profile']=='page':print(r)
for r in allocation_summary:print(r)
