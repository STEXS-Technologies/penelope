from pathlib import Path
import json,math,random,statistics
root=Path(__file__).resolve().parent;summary=[]
for campaign in range(2):
 rows=[json.loads(line) for line in (root/f'timing-{campaign}.jsonl').read_text().splitlines()];assert len(rows)==384
 assert all(r['campaign']==campaign and r['iterations']==(128 if r['size']==1048576 else 1000) and r['elapsed_ns']>0 for r in rows)
 for size in [0,32,4096,1048576]:
  for corrupt in [False,True]:
   ratios=[]
   for block in range(12):
    sample=[r for r in rows if r['size']==size and r['corrupt']==corrupt and r['block']==block]
    assert len(sample)==4 and sorted(r['position'] for r in sample)==[0,1,2,3]
    expected=['baseline','candidate','candidate','baseline'] if block%2==0 else ['candidate','baseline','baseline','candidate']
    assert [r['revision'] for r in sorted(sample,key=lambda r:r['position'])]==expected
    logs={label:[math.log(r['elapsed_ns']/r['iterations']) for r in sample if r['revision']==label] for label in ['baseline','candidate']}
    assert all(len(v)==2 for v in logs.values())
    ratios.append(statistics.mean(logs['candidate'])-statistics.mean(logs['baseline']))
   rng=random.Random(20261031+campaign*10000000+size*2+int(corrupt))
   bootstrap=sorted(statistics.mean(rng.choices(ratios,k=12)) for _ in range(20000))
   pct=lambda value:100*math.expm1(value)
   summary.append({'campaign':campaign,'size':size,'corrupt':corrupt,'blocks':12,'baseline_ns_per_call':math.exp(statistics.mean(math.log(r['elapsed_ns']/r['iterations']) for r in rows if r['size']==size and r['corrupt']==corrupt and r['revision']=='baseline')),'candidate_ns_per_call':math.exp(statistics.mean(math.log(r['elapsed_ns']/r['iterations']) for r in rows if r['size']==size and r['corrupt']==corrupt and r['revision']=='candidate')),'change_percent':pct(statistics.mean(ratios)),'bootstrap_95_percent':[pct(bootstrap[499]),pct(bootstrap[19499])]})
(root/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
for s in summary: print(s['campaign'],s['size'],s['corrupt'],round(s['change_percent'],4),[round(v,4) for v in s['bootstrap_95_percent']])
