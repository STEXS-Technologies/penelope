from pathlib import Path
import collections,json
root=Path(__file__).resolve().parent
result={}
for label in ['baseline','candidate']:
 counts=collections.Counter();values=collections.defaultdict(set)
 for repeat in range(4):
  rows=[json.loads(line) for line in (root/f'{label}-{repeat}.jsonl').read_text().splitlines()];assert len(rows)==64
  for actions in [1,16]:
   selected=[r for r in rows if r['actions']==actions];assert sorted(r['sample'] for r in selected)==list(range(32))
   for row in selected:
    counts[actions]+=1;values[actions].add(tuple(row[k] for k in ['allocations','reallocations','requested_bytes']))
 result[label]={}
 for actions in [1,16]:
  assert counts[actions]==128 and len(values[actions])==1
  result[label][str(actions)]=dict(zip(['allocations','reallocations','requested_bytes'],next(iter(values[actions]))),samples=counts[actions])
result['savings']={str(actions):{key:result['baseline'][str(actions)][key]-result['candidate'][str(actions)][key] for key in ['allocations','reallocations','requested_bytes']} for actions in [1,16]}
assert result['savings']=={'1':{'allocations':3,'reallocations':0,'requested_bytes':683},'16':{'allocations':33,'reallocations':0,'requested_bytes':6503}}
(root/'summary.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
