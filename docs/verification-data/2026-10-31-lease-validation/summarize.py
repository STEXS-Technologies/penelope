from pathlib import Path
import collections,json
root=Path(__file__).resolve().parent
cases=['outbox_ack','timer_ack','outbox_renew']; result={}
for label in ['baseline','candidate']:
 counts=collections.Counter(); values=collections.defaultdict(set)
 for repeat in range(4):
  rows=[json.loads(line) for line in (root/f'{label}-{repeat}.jsonl').read_text().splitlines()]
  assert len(rows)==96
  for case in cases:
   selected=[r for r in rows if r['case']==case];assert sorted(r['sample'] for r in selected)==list(range(32))
   for row in selected:
    counts[case]+=1;values[case].add(tuple(row[k] for k in ['allocations','reallocations','requested_bytes']))
 result[label]={}
 for case in cases:
  assert counts[case]==128 and len(values[case])==1
  result[label][case]=dict(zip(['allocations','reallocations','requested_bytes'],next(iter(values[case]))),samples=counts[case])
result['savings']={case:{key:result['baseline'][case][key]-result['candidate'][case][key] for key in ['allocations','reallocations','requested_bytes']} for case in cases}
assert result['savings']=={'outbox_ack':{'allocations':3,'reallocations':0,'requested_bytes':860},'timer_ack':{'allocations':2,'reallocations':0,'requested_bytes':689},'outbox_renew':{'allocations':3,'reallocations':0,'requested_bytes':860}}
(root/'summary.json').write_text(json.dumps(result,indent=2)+'\n')
print(json.dumps(result,indent=2))
