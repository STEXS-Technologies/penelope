from pathlib import Path
import sys,json,gzip,collections,statistics,random,math
out=Path(sys.argv[1]).resolve() if len(sys.argv)>1 else Path(__file__).resolve().parent
def read(name):
 p=out/name
 return p.read_text() if p.exists() else gzip.decompress((out/(name+'.gz')).read_bytes()).decode()
profiles={'one':(1,1),'page':(128,128),'stream':(4096,1),'rebuild':(4096,4096)}
rows=[json.loads(line) for line in read('raw.jsonl').splitlines()];assert len(rows)==768
keys=set();groups=collections.defaultdict(list);checksums={}
for r in rows:
 assert r['scope_len'] in [9,128] and r['id_len'] in [8,128] and r['profile'] in profiles
 assert (r['count'],r['chunk'])==profiles[r['profile']] and r['verified'] is True
 assert r['iterations']==(32 if r['count']>128 else 1000) and r['elapsed_ns']>0
 assert r['campaign'] in range(2) and r['block'] in range(6) and r['slot'] in range(4)
 order=['baseline','candidate','candidate','baseline'] if r['block']%2==0 else ['candidate','baseline','baseline','candidate']
 assert r['variant']==order[r['slot']]
 key=tuple(r[k] for k in ['campaign','block','slot','scope_len','id_len','profile']);assert key not in keys;keys.add(key)
 fixture=(r['scope_len'],r['id_len'],r['count'])
 assert checksums.setdefault(fixture,r['checksum'])==r['checksum']
 groups[(r['campaign'],r['scope_len'],r['id_len'],r['profile'])].append(r)
summaries=[]
for key,data in sorted(groups.items()):
 blocks=[]
 for block in range(6):
  values={v:[r['elapsed_ns']/r['iterations'] for r in data if r['block']==block and r['variant']==v] for v in ['baseline','candidate']}
  assert all(len(v)==2 for v in values.values())
  blocks.append(statistics.mean(map(math.log,values['candidate']))-statistics.mean(map(math.log,values['baseline'])))
 rng=random.Random(20261031)
 boot=sorted(math.expm1(statistics.mean(rng.choices(blocks,k=6)))*100 for _ in range(20000))
 summaries.append(dict(zip(['campaign','scope_len','id_len','profile'],key))|{'baseline_ns_per_log':statistics.median(r['elapsed_ns']/r['iterations'] for r in data if r['variant']=='baseline'),'candidate_ns_per_log':statistics.median(r['elapsed_ns']/r['iterations'] for r in data if r['variant']=='candidate'),'change_pct':math.expm1(statistics.mean(blocks))*100,'ci_low':boot[500],'ci_high':boot[19499]})
assert len(summaries)==32
alloc=[json.loads(line) for line in read('allocations.jsonl').splitlines()];assert len(alloc)==256
allocation=[]
for scope_len in [9,128]:
 for id_len in [8,128]:
  for profile,(count,chunk) in profiles.items():
   result=dict(scope_len=scope_len,id_len=id_len,profile=profile,count=count,chunk=chunk)
   for label in ['baseline','candidate']:
    subset=[r for r in alloc if (r['scope_len'],r['id_len'],r['profile'],r['variant'])==(scope_len,id_len,profile,label)]
    assert sorted(r['sample'] for r in subset)==list(range(8))
    assert all(r['verified'] and r['checksum']==checksums[(scope_len,id_len,count)] and (r['count'],r['chunk'])==(count,chunk) for r in subset)
    values={tuple(r[k] for k in ['allocations','reallocations','requested_bytes']) for r in subset};assert len(values)==1
    result[label]=dict(zip(['allocations','reallocations','requested_bytes'],values.pop()))
   result['removed_allocations']=result['baseline']['allocations']-result['candidate']['allocations']
   result['removed_requested_bytes']=result['baseline']['requested_bytes']-result['candidate']['requested_bytes']
   assert result['removed_allocations']==(count if chunk==1 else 0)
   assert result['removed_requested_bytes']==(116*count if chunk==1 else 0)
   assert result['baseline']['reallocations']==result['candidate']['reallocations']
   allocation.append(result)
assert sum(r['iterations'] for r in rows)==396288
assert sum(r['iterations']*r['count'] for r in rows)==75099648
assert sum(r['iterations']*(r['count']//r['chunk']) for r in rows)==25555968
(out/'summary.json').write_text(json.dumps({'latency':summaries,'allocation':allocation},indent=2)+'\n')
for r in summaries:print(r)
