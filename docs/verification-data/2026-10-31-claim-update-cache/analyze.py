from pathlib import Path
import json,random,statistics
out=Path(__file__).resolve().parent;runs=[json.loads(s) for s in (out/'runs.jsonl').read_text().splitlines()];assert len(runs)==96
stats=[]
for kind in ['empty','batch']:
 for timers in [False,True]:
  for batch in ([0] if kind=='empty' else [1,16]):
   changes=[];times={'baseline':[],'candidate':[]}
   for pair in range(24):
    values={r['variant']:next(x['elapsed_ns']/x['iterations'] for x in r['rows'] if x['timers']==timers and x.get('batch',0)==batch) for r in runs if r['pair']==pair and r['kind']==kind};assert len(values)==2
    changes.append(100*(values['candidate']/values['baseline']-1))
    for v in times:times[v].append(values[v])
   rng=random.Random(3102026+1000*(kind=='batch')+100*timers+batch);boot=sorted(statistics.mean(rng.choices(changes,k=24)) for _ in range(20000))
   stats.append(dict(kind=kind,timers=timers,batch=batch,pairs=24,baseline_ns=statistics.mean(times['baseline']),candidate_ns=statistics.mean(times['candidate']),change_percent=statistics.mean(changes),ci95=[boot[499],boot[19499]]))
(out/'analysis.json').write_text(json.dumps(stats,indent=2)+'\n')
for r in stats:print(r)
