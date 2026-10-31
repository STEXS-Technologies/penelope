from pathlib import Path
import collections,gzip,json,math,random,statistics,sys
out=Path(sys.argv[1]).resolve() if len(sys.argv)>1 else Path(__file__).resolve().parent
def read(name):
 p=out/name
 return p.read_text() if p.exists() else gzip.decompress((out/(name+'.gz')).read_bytes()).decode()
rows=[json.loads(line) for line in read('raw.jsonl').splitlines()]
assert len(rows)==1536
checksums={};keys=set();groups=collections.defaultdict(list)
for row in rows:
 assert type(row['long']) is bool and type(row['varying']) is bool
 assert row['outcomes'] in [1,128] and row['actions'] in [0,1,16,128]
 assert row['campaign'] in range(2) and row['block'] in range(6) and row['slot'] in range(4)
 order=['baseline','candidate','candidate','baseline'] if row['block']%2==0 else ['candidate','baseline','baseline','candidate']
 assert row['variant']==order[row['slot']]
 iterations={0:100000,1:100000,16:10000,128:1000}[row['actions']]
 if row['long']:iterations//=10
 if row['outcomes']==128:iterations=max(1,iterations//100)
 assert row['iterations']==iterations and row['elapsed_ns']>0
 fixture=tuple(row[k] for k in ['long','outcomes','varying','actions'])
 assert checksums.setdefault(fixture,row['checksum'])==row['checksum']
 key=tuple(row[k] for k in ['campaign','block','slot','long','outcomes','varying','actions']);assert key not in keys;keys.add(key)
 groups[(row['campaign'],*fixture)].append(row)
latency=[]
for key,data in sorted(groups.items()):
 blocks=[]
 for block in range(6):
  values={label:[r['elapsed_ns']/r['iterations'] for r in data if r['block']==block and r['variant']==label] for label in ['baseline','candidate']}
  assert all(len(v)==2 for v in values.values())
  blocks.append(statistics.mean(map(math.log,values['candidate']))-statistics.mean(map(math.log,values['baseline'])))
 rng=random.Random(20261031)
 boot=sorted(math.expm1(statistics.mean(rng.choices(blocks,k=6)))*100 for _ in range(20000))
 latency.append(dict(zip(['campaign','long','outcomes','varying','actions'],key))|{'baseline_ns':statistics.median(r['elapsed_ns']/r['iterations'] for r in data if r['variant']=='baseline'),'candidate_ns':statistics.median(r['elapsed_ns']/r['iterations'] for r in data if r['variant']=='candidate'),'change_pct':math.expm1(statistics.mean(blocks))*100,'ci_low':boot[500],'ci_high':boot[19499]})
assert len(latency)==64
alloc=[json.loads(line) for line in read('allocations.jsonl').splitlines()];assert len(alloc)==128
allocation=[]
for key,checksum in sorted(checksums.items()):
 result=dict(zip(['long','outcomes','varying','actions'],key))
 for label in ['baseline','candidate']:
  selected=[r for r in alloc if tuple(r[k] for k in ['long','outcomes','varying','actions'])==key and r['variant']==label]
  assert sorted(r['repeat'] for r in selected)==[0,1]
  assert all(r['checksum']==checksum for r in selected)
  values={tuple(r[k] for k in ['allocations','reallocations','deallocations','requested_bytes']) for r in selected};assert len(values)==1
  result[label]=dict(zip(['allocations','reallocations','deallocations','requested_bytes'],values.pop()))
  expected=4 if label=='baseline' and result['long'] else 0
  assert result[label]=={'allocations':expected,'reallocations':0,'deallocations':expected,'requested_bytes':expected*128}
 allocation.append(result)
assert len(allocation)==32
assert sum(r['iterations'] for r in rows)==22504416
measurement=json.loads(read('measurement.json'))
assert measurement['ordinary_processes']==384 and measurement['allocation_processes']==32
(out/'summary.json').write_text(json.dumps({'latency':latency,'allocation':allocation,'timed_validations':22504416},indent=2)+'\n')
for r in latency:print(r)
