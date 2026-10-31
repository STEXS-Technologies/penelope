from pathlib import Path
import collections,gzip,json,math,random,statistics,sys
family=sys.argv[1];out=Path(sys.argv[2]);assert family in ['private','public']
def read(name):
 p=out/name
 return p.read_bytes() if p.exists() else gzip.decompress((out/(name+'.gz')).read_bytes())
rows=[json.loads(line) for line in read('raw.jsonl').decode().splitlines()]
private=family=='private';blocks=8 if private else 6
profiles={(size,case) for size in [0,32,4096,1048576] for case in ['duplicate','read','missing','corrupt_read','corrupt_put']} if private else {(batch,width) for batch in [1,16,128] for width in [9,128]}
assert len(rows)==(1280 if private else 576)
key=lambda row: (row['size'],row['case']) if private else (row['batch'],row['width'])
identities=set();checksums=collections.defaultdict(set)
for r in rows:
 c,b,s=r['campaign'],r['block'],r['slot'];assert c in range(2) and b in range(blocks) and s in range(4) and key(r) in profiles
 order=['baseline','candidate','candidate','baseline'] if b%2==0 else ['candidate','baseline','baseline','candidate'];assert r['variant']==order[s]
 identity=(c,b,s,key(r)) if private else (c,b,s,key(r),r['round']);assert identity not in identities;identities.add(identity)
 if private:assert r['iterations']==(128 if r['size']==1048576 else 1000) and r['elapsed_ns']>0
 else:
  assert r['round'] in range(2) and r['direct_first']==(r['round']==0) and r['work']==65536 and r['reopen'] is False
  assert r['schema']==1 and r['verified'] and r['consumed']==r['batch'] and r['outcomes']==r['batch']+1 and r['reopen_ns']==0
  assert r['managed_ns']==sum(r[n] for n in ['schedule_ns','delivery_ns','reopen_ns','consume_ns'])
  checksums[key(r)].add(r['checksum'])
assert all(len(values)==1 for values in checksums.values())
latency=[]
for c in range(2):
 for profile in sorted(profiles):
  for metric in (['call_ns'] if private else ['managed_ns','schedule_ns','delivery_ns','consume_ns','direct_ns']):
   get=lambda r:r['elapsed_ns']/r['iterations'] if private else r[metric]
   sample=[r for r in rows if r['campaign']==c and key(r)==profile];logs=[]
   for b in range(blocks):
    groups={v:[math.log(get(r)) for r in sample if r['block']==b and r['variant']==v] for v in ['baseline','candidate']}
    assert all(len(g)==(2 if private else 4) for g in groups.values())
    logs.append(statistics.mean(groups['candidate'])-statistics.mean(groups['baseline']))
   rng=random.Random(f'20261031-{family}-{c}-{profile}-{metric}');boot=sorted(statistics.mean(rng.choices(logs,k=blocks)) for _ in range(20000));pct=lambda v:100*math.expm1(v)
   dimensions=dict(size=profile[0],case=profile[1]) if private else dict(batch=profile[0],width=profile[1])
   latency.append(dimensions|dict(campaign=c,metric=metric,baseline_ns=math.exp(statistics.mean(math.log(get(r)) for r in sample if r['variant']=='baseline')),candidate_ns=math.exp(statistics.mean(math.log(get(r)) for r in sample if r['variant']=='candidate')),change_pct=pct(statistics.mean(logs)),ci_low=pct(boot[500]),ci_high=pct(boot[19499])))
allocations=[]
if private:
 data=[json.loads(line) for line in read('allocations.jsonl').decode().splitlines()];assert len(data)==2560
 seen=set()
 for r in data:
  identity=(r['variant'],r['repeat'],key(r),r['sample']);assert identity not in seen;seen.add(identity)
  assert r['variant'] in ['baseline','candidate'] and r['repeat'] in range(4) and key(r) in profiles and r['sample'] in range(16)
 for profile in sorted(profiles):
  groups={v:[r for r in data if key(r)==profile and r['variant']==v] for v in ['baseline','candidate']}
  dimensions=dict(size=profile[0],case=profile[1]);measurements={v:{name:dict(median=statistics.median(r[name] for r in sample),min=min(r[name] for r in sample),max=max(r[name] for r in sample)) for name in ['allocations','reallocations','requested_bytes']} for v,sample in groups.items()}
  allocations.append(dimensions|measurements)
m=json.loads(read('measurement.json'));assert (m['ordinary_processes'],m['allocation_processes'])==((64,8) if private else (288,0));assert 'btrfs' in m['filesystem']
summary=dict(latency=latency,allocations=allocations,rows=len(rows),timers=0 if private else sum(r['consumed'] for r in rows))
(out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
for metric in (['call_ns'] if private else ['managed_ns','delivery_ns']):
 for case in (['duplicate','corrupt_put','read','missing','corrupt_read'] if private else ['all']):
  data=[r for r in latency if r['metric']==metric and (not private or r['case']==case)]
  print(family,metric,case,'gains',sum(r['ci_high']<0 for r in data),'regressions',sum(r['ci_low']>0 for r in data),'range',min(r['change_pct'] for r in data),max(r['change_pct'] for r in data))
for r in latency:
 if r['ci_low']>0:print('regression',r)
