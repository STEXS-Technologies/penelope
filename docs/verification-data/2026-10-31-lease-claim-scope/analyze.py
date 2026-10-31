from pathlib import Path
import collections,gzip,json,math,random,statistics,sys
out=Path(sys.argv[1]).resolve() if len(sys.argv)>1 else Path(__file__).resolve().parent
def read(name):
 p=out/name
 return p.read_text() if p.exists() else gzip.decompress((out/(name+'.gz')).read_bytes()).decode()
profiles={(t,l,b) for t in [False,True] for l in [9,128] for b in [1,16,128]}
rows=[json.loads(line) for line in read('raw.jsonl').splitlines()];assert len(rows)==4608
checksums={};keys=set();groups=collections.defaultdict(list)
for row in rows:
 profile=tuple(row[k] for k in ['timers','length','batch']);assert profile in profiles
 assert row['campaign'] in range(2) and row['block'] in range(6) and row['slot'] in range(4) and row['sample'] in range(2,10)
 order=['baseline','candidate','candidate','baseline'] if row['block']%2==0 else ['candidate','baseline','baseline','candidate']
 assert row['variant']==order[row['slot']] and row['elapsed_ns']>0 and row['verified'] is True
 fixture=(*profile,row['sample']);assert checksums.setdefault(fixture,row['checksum'])==row['checksum']
 key=(row['campaign'],row['block'],row['slot'],*fixture);assert key not in keys;keys.add(key)
 groups[(row['campaign'],*profile)].append(row)
latency=[]
for key,data in sorted(groups.items()):
 blocks=[]
 for block in range(6):
  values={label:[math.log(r['elapsed_ns']) for r in data if r['block']==block and r['variant']==label] for label in ['baseline','candidate']}
  assert all(len(values[label])==16 for label in values)
  blocks.append(statistics.mean(values['candidate'])-statistics.mean(values['baseline']))
 rng=random.Random(20261031);boot=sorted(math.expm1(statistics.mean(rng.choices(blocks,k=6)))*100 for _ in range(20000))
 latency.append(dict(zip(['campaign','timers','length','batch'],key))|{'baseline_ns':statistics.median(r['elapsed_ns'] for r in data if r['variant']=='baseline'),'candidate_ns':statistics.median(r['elapsed_ns'] for r in data if r['variant']=='candidate'),'change_pct':math.expm1(statistics.mean(blocks))*100,'ci_low':boot[500],'ci_high':boot[19499]})
assert len(latency)==24
alloc=[json.loads(line) for line in read('allocations.jsonl').splitlines()];assert len(alloc)==576
keys=set();allocation=[]
for row in alloc:
 profile=tuple(row[k] for k in ['timers','length','batch']);assert profile in profiles
 assert row['variant'] in ['pre-port','baseline','candidate'] and row['repeat'] in range(2) and row['sample'] in range(2,10) and row['verified'] is True
 fixture=(*profile,row['sample']);assert checksums[fixture]==row['checksum']
 key=(row['repeat'],row['variant'],*fixture);assert key not in keys;keys.add(key)
for profile in sorted(profiles):
 result=dict(zip(['timers','length','batch'],profile))
 for variant in ['pre-port','baseline','candidate']:
  data=[r for r in alloc if tuple(r[k] for k in ['timers','length','batch'])==profile and r['variant']==variant];assert len(data)==16
  result[variant]={name:{'min':min(r[name] for r in data),'median':statistics.median(r[name] for r in data),'max':max(r[name] for r in data)} for name in ['allocations','reallocations','requested_bytes']}
  assert result[variant]['allocations']['max']-result[variant]['allocations']['min']<=2
  assert result[variant]['requested_bytes']['max']-result[variant]['requested_bytes']['min']<=144
  assert result[variant]['reallocations']['min']==result[variant]['reallocations']['max']
 timers,length,batch=profile
 for before,after,multiplier in [('pre-port','baseline',int(timers)),('baseline','candidate',1)]:
  for metric,factor in [('allocations',4),('requested_bytes',512),('reallocations',0)]:
   expected=multiplier*factor*batch if length==128 else 0
   assert result[before][metric]['min']-result[after][metric]['min']==expected
   # Medians are retained; occasional two-allocation differences are not hidden.
 allocation.append(result)
measurement=json.loads(read('measurement.json'));assert (measurement['ordinary_processes'],measurement['allocation_processes'])==(48,6)
assert sum(r['batch'] for r in rows)==222720
(out/'summary.json').write_text(json.dumps({'latency':latency,'allocation':allocation,'timed_claims':4608,'timed_envelopes':222720},indent=2)+'\n')
for row in latency:print(row)
