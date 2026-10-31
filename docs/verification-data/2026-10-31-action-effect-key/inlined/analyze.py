from pathlib import Path
import collections,gzip,json,math,random,statistics,sys
out=Path(sys.argv[1]).resolve() if len(sys.argv)>1 else Path(__file__).resolve().parent
def read(name):
 p=out/name
 return p.read_text() if p.exists() else gzip.decompress((out/(name+'.gz')).read_bytes()).decode()
lengths={'short':124,'inline-limit':189,'heap-first':194,'maximum':719,'mixed':321}
def summarize(rows,dimensions,family):
 groups=collections.defaultdict(list);keys=set();checksums={}
 for row in rows:
  profile=tuple(row[k] for k in dimensions)
  assert row['campaign'] in range(2) and row['block'] in range(6) and row['slot'] in range(4)
  order=['baseline','candidate','candidate','baseline'] if row['block']%2==0 else ['candidate','baseline','baseline','candidate']
  assert row['variant']==order[row['slot']] and row['elapsed_ns']>0
  if family=='domain':
   assert row['profile'] in lengths and row['kind'] in range(4) and row['path'] in ['key-control','action'] and row['iterations']==100000 and row['encoded_len']==lengths[row['profile']]
   sample=0;fixture=(row['profile'],row['kind'])
  else:
   assert row['timers'] in [False,True] and row['length'] in [9,128] and row['batch'] in [1,16,128] and row['sample'] in range(2,10) and row['verified'] is True
   sample=row['sample'];fixture=(*profile,sample)
  assert checksums.setdefault(fixture,row['checksum'])==row['checksum']
  key=(row['campaign'],row['block'],row['slot'],*profile,sample);assert key not in keys;keys.add(key)
  groups[(row['campaign'],*profile)].append(row)
 result=[]
 for key,data in sorted(groups.items()):
  blocks=[]
  def cost(r):return r['elapsed_ns']/r.get('iterations',1)
  for block in range(6):
   values={v:[math.log(cost(r)) for r in data if r['block']==block and r['variant']==v] for v in ['baseline','candidate']}
   assert all(len(values[v])==(2 if family=='domain' else 16) for v in values)
   blocks.append(statistics.mean(values['candidate'])-statistics.mean(values['baseline']))
  rng=random.Random(20261031);boot=sorted(math.expm1(statistics.mean(rng.choices(blocks,k=6)))*100 for _ in range(20000))
  result.append(dict(zip(['campaign']+dimensions,key))|{'baseline_ns':statistics.median(cost(r) for r in data if r['variant']=='baseline'),'candidate_ns':statistics.median(cost(r) for r in data if r['variant']=='candidate'),'change_pct':math.expm1(statistics.mean(blocks))*100,'ci_low':boot[500],'ci_high':boot[19499]})
 return result,checksums
rows=[json.loads(s) for s in read('domain-raw.jsonl').splitlines()];assert len(rows)==1920 and sum(r['iterations'] for r in rows)==192000000
domain,domain_checksums=summarize(rows,['profile','kind','path'],'domain');assert len(domain)==80
rows=[json.loads(s) for s in read('public-raw.jsonl').splitlines()];assert len(rows)==4608 and sum(r['batch'] for r in rows)==222720
public,public_checksums=summarize(rows,['timers','length','batch'],'public');assert len(public)==24
alloc=[json.loads(s) for s in read('domain-allocations.jsonl').splitlines()];assert len(alloc)==160
keys=set()
cloned={'short':0,'inline-limit':0,'heap-first':115,'maximum':640,'mixed':215}
counts={'short':0,'inline-limit':0,'heap-first':5,'maximum':5,'mixed':3}
for row in alloc:
 assert row['profile'] in lengths and row['kind'] in range(4) and row['path'] in ['key-control','action'] and row['variant'] in ['baseline','candidate'] and row['repeat'] in range(2)
 key=tuple(row[k] for k in ['profile','kind','path','variant','repeat']);assert key not in keys;keys.add(key)
 assert row['checksum']==domain_checksums[(row['profile'],row['kind'])] and row['encoded_len']==lengths[row['profile']]
 owned=row['variant']=='baseline' and row['path']=='action'
 assert (row['allocations'],row['reallocations'],row['requested_bytes'])==(1+(counts[row['profile']] if owned else 0),0,lengths[row['profile']]+(cloned[row['profile']] if owned else 0))
public_alloc=[json.loads(s) for s in read('public-allocations.jsonl').splitlines()];assert len(public_alloc)==384
keys=set();allocation=[]
for row in public_alloc:
 profile=tuple(row[k] for k in ['timers','length','batch']);assert row['variant'] in ['baseline','candidate'] and row['repeat'] in range(2) and row['sample'] in range(2,10) and row['verified'] is True
 fixture=(*profile,row['sample']);assert row['checksum']==public_checksums[fixture]
 key=(row['repeat'],row['variant'],*fixture);assert key not in keys;keys.add(key)
for timers in [False,True]:
 for length in [9,128]:
  for batch in [1,16,128]:
   result={'timers':timers,'length':length,'batch':batch}
   for variant in ['baseline','candidate']:
    data=[r for r in public_alloc if (r['timers'],r['length'],r['batch'],r['variant'])==(timers,length,batch,variant)];assert len(data)==16
    result[variant]={name:{'min':min(r[name] for r in data),'median':statistics.median(r[name] for r in data),'max':max(r[name] for r in data)} for name in ['allocations','reallocations','requested_bytes']}
   allocation.append(result)
measurement=json.loads(read('measurement.json'));assert (measurement['ordinary_processes'],measurement['allocation_processes'])==(96,8)
(out/'summary.json').write_text(json.dumps({'domain_latency':domain,'domain_allocation':alloc,'public_latency':public,'public_allocation':allocation,'timed_encodings':192000000,'timed_claims':4608,'timed_envelopes':222720},indent=2)+'\n')
for label,rs in [('control',[r for r in domain if r['path']=='key-control']),('action',[r for r in domain if r['path']=='action']),('public',public)]:
 print(label,'gains',sum(r['ci_high']<0 for r in rs),'regressions',sum(r['ci_low']>0 for r in rs))
 for row in rs:
  if row['ci_low']>0:print(row)
