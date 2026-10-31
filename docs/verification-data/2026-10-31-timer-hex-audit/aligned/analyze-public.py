from pathlib import Path
import collections,gzip,json,math,random,statistics,sys
out=Path(sys.argv[1]).resolve() if len(sys.argv)>1 else Path(__file__).resolve().parent
def read(name):
 p=out/name
 return p.read_text() if p.exists() else gzip.decompress((out/(name+'.gz')).read_bytes()).decode()
dimensions=['batch','width','order','timers'];profiles={(n,w,o,t) for n in [1,8,16,32,64,128] for w in [9,128] for o in [0,1,2] for t in [False,True]}
rows=[json.loads(s) for s in read('raw.jsonl').splitlines()];assert len(rows)==3456 and sum(r['iterations'] for r in rows)==110592
seen=set();checksums={};groups=collections.defaultdict(list)
for r in rows:
 profile=tuple(r[k] for k in dimensions);assert profile in profiles and r['iterations']==32 and r['elapsed_ns']>0
 assert r['campaign'] in range(2) and r['block'] in range(6) and r['slot'] in range(4)
 order=['baseline','candidate','candidate','baseline'] if r['block']%2==0 else ['candidate','baseline','baseline','candidate'];assert r['variant']==order[r['slot']]
 key=(r['campaign'],r['block'],r['slot'],*profile);assert key not in seen;seen.add(key)
 assert checksums.setdefault(profile,r['checksum'])==r['checksum'];groups[(r['campaign'],*profile)].append(r)
latency=[]
for key,data in sorted(groups.items()):
 blocks=[]
 for block in range(6):
  values={v:[math.log(r['elapsed_ns']/r['iterations']) for r in data if r['block']==block and r['variant']==v] for v in ['baseline','candidate']};assert all(len(x)==2 for x in values.values())
  blocks.append(statistics.mean(values['candidate'])-statistics.mean(values['baseline']))
 rng=random.Random(20261031);boot=sorted(100*math.expm1(statistics.mean(rng.choices(blocks,k=6))) for _ in range(20000))
 latency.append(dict(zip(['campaign']+dimensions,key))|{'baseline_ns':statistics.median(r['elapsed_ns']/r['iterations'] for r in data if r['variant']=='baseline'),'candidate_ns':statistics.median(r['elapsed_ns']/r['iterations'] for r in data if r['variant']=='candidate'),'change_pct':100*math.expm1(statistics.mean(blocks)),'ci_low':boot[500],'ci_high':boot[19499]})
assert len(latency)==144
alloc=[json.loads(s) for s in read('allocations.jsonl').splitlines()];assert len(alloc)==288
seen=set();allocations=[]
for r in alloc:
 profile=tuple(r[k] for k in dimensions);assert profile in profiles and r['variant'] in ['baseline','candidate'] and r['repeat'] in range(2)
 key=(r['repeat'],r['variant'],*profile);assert key not in seen;seen.add(key);assert r['checksum']==checksums[profile]
for profile in sorted(profiles):
 result=dict(zip(dimensions,profile))
 for variant in ['baseline','candidate']:
  values={tuple(r[k] for k in ['allocations','reallocations','requested_bytes']) for r in alloc if tuple(r[k] for k in dimensions)==profile and r['variant']==variant};assert len(values)==1
  result[variant]=dict(zip(['allocations','reallocations','requested_bytes'],values.pop()))
 assert result['baseline']==result['candidate'];allocations.append(result)
measurement=json.loads(read('measurement.json'));assert (measurement['ordinary_processes'],measurement['allocation_processes'])==(96,8)
(out/'summary.json').write_text(json.dumps({'latency':latency,'allocation':allocations,'timed_public_calls':110592},indent=2)+'\n')
for timers in [False,True]:
 for large in [False,True]:
  data=[r for r in latency if r['timers']==timers and (r['batch']>16)==large]
  print('timers',timers,'large',large,'gains',sum(r['ci_high']<0 for r in data),'regressions',sum(r['ci_low']>0 for r in data),'range',min(r['change_pct'] for r in data),max(r['change_pct'] for r in data))
for r in latency:
 if r['ci_low']>0:print('regression',r)
