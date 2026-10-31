from pathlib import Path
import collections,gzip,json,math,random,statistics,sys
out=Path(sys.argv[1]).resolve() if len(sys.argv)>1 else Path(__file__).resolve().parent
def read(name):
 p=out/name
 return p.read_text() if p.exists() else gzip.decompress((out/(name+'.gz')).read_bytes()).decode()
profiles={f'scope-{scope}-step-{step}-action-{action}' for scope in [5,22,23,128] for step in [5,128] for action in [5,128]}
rows=[json.loads(s) for s in read('raw.jsonl').splitlines()];assert len(rows)==768 and sum(r['iterations'] for r in rows)==7680000
seen=set();identity={};groups=collections.defaultdict(list)
for r in rows:
 assert r['profile'] in profiles and r['iterations']==10000 and r['elapsed_ns']>0
 assert r['campaign'] in range(2) and r['block'] in range(6) and r['slot'] in range(4)
 order=['baseline','candidate','candidate','baseline'] if r['block']%2==0 else ['candidate','baseline','baseline','candidate'];assert r['variant']==order[r['slot']]
 key=tuple(r[k] for k in ['campaign','block','slot','profile']);assert key not in seen;seen.add(key)
 value=tuple(r[k] for k in ['checksum','input_id','payload_len']);assert identity.setdefault(r['profile'],value)==value
 assert len(r['input_id'])==74 and r['input_id'].startswith('inp_timer_')
 groups[(r['campaign'],r['profile'])].append(r)
latency=[]
for (campaign,profile),data in sorted(groups.items()):
 blocks=[]
 for block in range(6):
  values={v:[math.log(r['elapsed_ns']/r['iterations']) for r in data if r['block']==block and r['variant']==v] for v in ['baseline','candidate']};assert all(len(x)==2 for x in values.values())
  blocks.append(statistics.mean(values['candidate'])-statistics.mean(values['baseline']))
 rng=random.Random(20261031);boot=sorted(100*math.expm1(statistics.mean(rng.choices(blocks,k=6))) for _ in range(20000))
 latency.append({'campaign':campaign,'profile':profile,'baseline_ns':statistics.median(r['elapsed_ns']/r['iterations'] for r in data if r['variant']=='baseline'),'candidate_ns':statistics.median(r['elapsed_ns']/r['iterations'] for r in data if r['variant']=='candidate'),'change_pct':100*math.expm1(statistics.mean(blocks)),'ci_low':boot[500],'ci_high':boot[19499]})
assert len(latency)==32
allocations=[json.loads(s) for s in read('allocations.jsonl').splitlines()];assert len(allocations)==64
seen=set();allocation=[]
for r in allocations:
 assert r['profile'] in profiles and r['variant'] in ['baseline','candidate'] and r['repeat'] in range(2)
 key=tuple(r[k] for k in ['repeat','variant','profile']);assert key not in seen;seen.add(key)
 assert tuple(r[k] for k in ['checksum','input_id','payload_len'])==identity[r['profile']]
for profile in sorted(profiles):
 result={'profile':profile}
 for variant in ['baseline','candidate']:
  values={tuple(r[k] for k in ['allocations','reallocations','requested_bytes']) for r in allocations if r['profile']==profile and r['variant']==variant};assert len(values)==1
  result[variant]=dict(zip(['allocations','reallocations','requested_bytes'],values.pop()))
 assert result['baseline']['allocations']==result['candidate']['allocations']
 assert result['baseline']==result['candidate']
 allocation.append(result)
measurement=json.loads(read('measurement.json'));assert (measurement['ordinary_processes'],measurement['allocation_processes'])==(48,4)
(out/'summary.json').write_text(json.dumps({'latency':latency,'allocation':allocation,'timed_calls':7680000},indent=2)+'\n')
print('gains',sum(r['ci_high']<0 for r in latency),'regressions',sum(r['ci_low']>0 for r in latency),'range',min(r['change_pct'] for r in latency),max(r['change_pct'] for r in latency))
for r in latency:print(r)
