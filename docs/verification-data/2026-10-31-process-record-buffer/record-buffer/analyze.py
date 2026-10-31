from pathlib import Path
import collections,gzip,json,math,random,statistics,sys
out=Path(sys.argv[1]).resolve() if len(sys.argv)>1 else Path(__file__).resolve().parent
def read(name):
 p=out/name
 return p.read_text() if p.exists() else gzip.decompress((out/(name+'.gz')).read_bytes()).decode()
rows=[json.loads(line) for line in read('raw.jsonl').splitlines()];assert len(rows)==3072
checksums={};keys=set();groups=collections.defaultdict(list)
for row in rows:
 assert type(row['long']) is bool and row['outcomes'] in [1,128] and row['actions'] in [0,128]
 assert row['sample'] in range(2,10) and row['campaign'] in range(2) and row['block'] in range(6) and row['slot'] in range(4)
 order=['baseline','candidate','candidate','baseline'] if row['block']%2==0 else ['candidate','baseline','baseline','candidate']
 assert row['variant']==order[row['slot']] and row['measurement']['elapsed_ns']>0
 fixture=tuple(row[k] for k in ['long','outcomes','actions','sample']);assert checksums.setdefault(fixture,row['checksum'])==row['checksum']
 key=tuple(row[k] for k in ['campaign','block','slot','long','outcomes','actions','sample']);assert key not in keys;keys.add(key)
 groups[tuple(row[k] for k in ['campaign','long','outcomes','actions'])].append(row)
latency=[]
for key,data in sorted(groups.items()):
 blocks=[]
 for block in range(6):
  values={label:[r['measurement']['elapsed_ns'] for r in data if r['block']==block and r['variant']==label] for label in ['baseline','candidate']}
  assert all(len(values[label])==16 for label in values)
  blocks.append(statistics.mean(map(math.log,values['candidate']))-statistics.mean(map(math.log,values['baseline'])))
 rng=random.Random(20261031);boot=sorted(math.expm1(statistics.mean(rng.choices(blocks,k=6)))*100 for _ in range(20000))
 latency.append(dict(zip(['campaign','long','outcomes','actions'],key))|{'baseline_ns':statistics.median(r['measurement']['elapsed_ns'] for r in data if r['variant']=='baseline'),'candidate_ns':statistics.median(r['measurement']['elapsed_ns'] for r in data if r['variant']=='candidate'),'change_pct':math.expm1(statistics.mean(blocks))*100,'ci_low':boot[500],'ci_high':boot[19499]})
assert len(latency)==16
alloc=[json.loads(line) for line in read('allocations.jsonl').splitlines()];assert len(alloc)==256
allocation=[];keys=set();agroups=collections.defaultdict(list)
for row in alloc:
 fixture=tuple(row[k] for k in ['long','outcomes','actions','sample']);assert checksums[fixture]==row['checksum']
 assert row['repeat'] in [0,1] and row['variant'] in ['baseline','candidate']
 key=tuple(row[k] for k in ['repeat','variant','long','outcomes','actions','sample']);assert key not in keys;keys.add(key)
 agroups[tuple(row[k] for k in ['long','outcomes','actions'])].append(row)
for key,data in sorted(agroups.items()):
 result=dict(zip(['long','outcomes','actions'],key))
 for variant in ['baseline','candidate']:
  selected=[r['measurement'] for r in data if r['variant']==variant];assert len(selected)==16
  result[variant]={name:{'min':min(r[name] for r in selected),'median':statistics.median(r[name] for r in selected),'max':max(r[name] for r in selected)} for name in ['allocations','reallocations','requested_bytes']}
 for name in ['allocations','reallocations','requested_bytes']:
  assert result['candidate'][name]['max']<=result['baseline'][name]['min'],(key,name,result)
 allocation.append(result)
assert len(allocation)==8
(out/'summary.json').write_text(json.dumps({'latency':latency,'allocation':allocation,'timed_commits':3072},indent=2)+'\n')
for r in latency:print(r)
for r in allocation:print(r)
