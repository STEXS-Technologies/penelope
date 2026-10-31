from pathlib import Path
import collections,json
root=Path(__file__).resolve().parent;result={}
for label in ['baseline','candidate']:
 counts=collections.Counter();values=collections.defaultdict(set)
 for repeat in range(4):
  rows=[json.loads(line) for line in (root/f'{label}-{repeat}.jsonl').read_text().splitlines()];assert len(rows)==96
  for pending in [0,1,16]:
   selected=[r for r in rows if r['pending']==pending];assert sorted(r['sample'] for r in selected)==list(range(32))
   for row in selected:
    counts[pending]+=1;values[pending].add(tuple(row[k] for k in ['allocations','reallocations','requested_bytes']))
 result[label]={}
 for pending in [0,1,16]:
  assert counts[pending]==128 and len(values[pending])==1
  result[label][str(pending)]=dict(zip(['allocations','reallocations','requested_bytes'],next(iter(values[pending]))),samples=counts[pending])
result['savings']={str(pending):{key:result['baseline'][str(pending)][key]-result['candidate'][str(pending)][key] for key in ['allocations','reallocations','requested_bytes']} for pending in [0,1,16]}
assert result['savings']=={'0':{'allocations':0,'reallocations':0,'requested_bytes':1648},'1':{'allocations':1,'reallocations':0,'requested_bytes':66},'16':{'allocations':31,'reallocations':0,'requested_bytes':3216}}
(root/'summary.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
