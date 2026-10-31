from pathlib import Path
import collections,json
root=Path(__file__).resolve().parent;result={}
for label in ['baseline','candidate']:
 counts=collections.Counter();values=collections.defaultdict(set)
 for repeat in range(4):
  rows=[json.loads(line) for line in (root/f'{label}-{repeat}.jsonl').read_text().splitlines()];assert len(rows)==64
  for consumed in [False,True]:
   selected=[r for r in rows if r['consumed']==consumed];assert sorted(r['sample'] for r in selected)==list(range(32))
   for row in selected:
    counts[consumed]+=1;values[consumed].add(tuple(row[k] for k in ['allocations','reallocations','requested_bytes']))
 result[label]={}
 for consumed in [False,True]:
  assert counts[consumed]==128 and len(values[consumed])==1
  result[label][str(consumed)]=dict(zip(['allocations','reallocations','requested_bytes'],next(iter(values[consumed]))),samples=counts[consumed])
result['savings']={str(consumed):{key:result['baseline'][str(consumed)][key]-result['candidate'][str(consumed)][key] for key in ['allocations','reallocations','requested_bytes']} for consumed in [False,True]}
assert all(row=={'allocations':1,'reallocations':0,'requested_bytes':116} for row in result['savings'].values())
(root/'summary.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
