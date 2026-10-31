from pathlib import Path
import json,math,random,statistics
root=Path(__file__).resolve().parent;summary=[]
for campaign in range(2):
 rows=[json.loads(line) for line in (root/f'timing-{campaign}.jsonl').read_text().splitlines()];assert len(rows)==144
 assert all(r['campaign']==campaign and r['iterations']==1000 and r['elapsed_ns']>0 and r['profile'] in ['record','padded','end'] for r in rows)
 for profile_index,profile in enumerate(['record','padded','end']):
  ratios=[]
  for block in range(12):
   sample=[r for r in rows if r['profile']==profile and r['block']==block];assert len(sample)==4 and sorted(r['position'] for r in sample)==[0,1,2,3]
   expected=['baseline','candidate','candidate','baseline'] if block%2==0 else ['candidate','baseline','baseline','candidate']
   assert [r['revision'] for r in sorted(sample,key=lambda r:r['position'])]==expected
   logs={label:[math.log(r['elapsed_ns']/r['iterations']) for r in sample if r['revision']==label] for label in ['baseline','candidate']}
   assert all(len(v)==2 for v in logs.values())
   ratios.append(statistics.mean(logs['candidate'])-statistics.mean(logs['baseline']))
  rng=random.Random(20261031+campaign*2+profile_index);bootstrap=sorted(statistics.mean(rng.choices(ratios,k=12)) for _ in range(20000))
  pct=lambda value:100*math.expm1(value)
  summary.append({'campaign':campaign,'profile':profile,'blocks':12,'baseline_ns_per_call':math.exp(statistics.mean(math.log(r['elapsed_ns']/r['iterations']) for r in rows if r['profile']==profile and r['revision']=='baseline')),'candidate_ns_per_call':math.exp(statistics.mean(math.log(r['elapsed_ns']/r['iterations']) for r in rows if r['profile']==profile and r['revision']=='candidate')),'change_percent':pct(statistics.mean(ratios)),'bootstrap_95_percent':[pct(bootstrap[499]),pct(bootstrap[19499])]})
(root/'timing-summary.json').write_text(json.dumps(summary,indent=2)+'\n');print(json.dumps(summary,indent=2))
