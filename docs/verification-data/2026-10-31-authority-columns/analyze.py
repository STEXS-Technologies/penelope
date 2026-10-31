from pathlib import Path
import json,random,statistics
out=Path(__file__).resolve().parent
rows=[json.loads(l) for l in (out/'runs.jsonl').read_text().splitlines()]
assert len(rows)==24*2*3*17
summary={}
rng=random.Random(20261031)
for profile in [2,16,128]:
 points={v:sorted([r for r in rows if r['mode']=='latency' and r['history']==profile and r['variant']==v],key=lambda r:r['pair']) for v in ['baseline','candidate']}
 assert all(len(v)==24 for v in points.values())
 deltas=[100*(c['elapsed_ns']/b['elapsed_ns']-1) for b,c in zip(points['baseline'],points['candidate'])]
 boot=sorted(statistics.mean(rng.choices(deltas,k=24)) for _ in range(20000))
 allocation={}
 for variant in ['baseline','candidate']:
  samples=[r for r in rows if r['history']==profile and r['mode']=='allocations' and r['variant']==variant]
  assert len(samples)==384
  values={tuple(r[k] for k in ['allocations','reallocations','requested_bytes']) for r in samples}
  assert len(values)==1,values
  allocation[variant]=list(values.pop())
 summary[profile]={'paired_mean_percent':statistics.mean(deltas),'bootstrap_95_percent':[boot[499],boot[19499]],'mean_ns_per_read':{v:statistics.mean(r['elapsed_ns']/r['iterations'] for r in points[v]) for v in points},'allocation_traffic_alloc_realloc_bytes':allocation}
for count,item in summary.items():
 b=item['allocation_traffic_alloc_realloc_bytes']['baseline'];c=item['allocation_traffic_alloc_realloc_bytes']['candidate']
 assert b[0]-c[0]==2 and b[2]-c[2]==64,(count,b,c)
 assert c==[0,0,0]
(out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n');print(json.dumps(summary,indent=2))
