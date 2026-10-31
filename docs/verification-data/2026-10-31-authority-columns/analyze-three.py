from pathlib import Path
import json,statistics,random
out=Path(__file__).resolve().parent;rows=[json.loads(l) for l in (out/'final-runs.jsonl').read_text().splitlines()];assert len(rows)==24*3*3*17
rng=random.Random(20261031);summary={}
for history in [2,16,128]:
 points={v:sorted([r for r in rows if r['history']==history and r['mode']=='latency' and r['variant']==v],key=lambda r:r['block']) for v in ['baseline','uncached','cached']};assert all(len(p)==24 for p in points.values())
 allocation={}
 for v in points:
  samples=[r for r in rows if r['history']==history and r['mode']=='allocations' and r['variant']==v];assert len(samples)==384
  values={tuple(r[k] for k in ['allocations','reallocations','requested_bytes']) for r in samples};assert len(values)==1;allocation[v]=list(values.pop())
 assert allocation=={'baseline':[2,0,64],'uncached':[0,0,0],'cached':[1,0,96]}
 comparisons={}
 for base,candidate in [('baseline','uncached'),('baseline','cached'),('uncached','cached')]:
  delta=[100*(c['elapsed_ns']/b['elapsed_ns']-1) for b,c in zip(points[base],points[candidate])]
  boot=sorted(statistics.mean(rng.choices(delta,k=24)) for _ in range(20000))
  comparisons[base+'-to-'+candidate]={'paired_mean_percent':statistics.mean(delta),'bootstrap_95_percent':[boot[499],boot[19499]]}
 summary[history]={'comparisons':comparisons,'mean_ns_per_retry':{v:statistics.mean(r['elapsed_ns']/r['iterations'] for r in points[v]) for v in points},'allocation_traffic_alloc_realloc_bytes':allocation}
(out/'final-summary.json').write_text(json.dumps(summary,indent=2)+'\n');print(json.dumps(summary,indent=2))
