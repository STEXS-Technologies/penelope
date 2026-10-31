from pathlib import Path
import itertools,json
out=Path(__file__).resolve().parent
runs=[json.loads(line) for line in (out/'allocations.jsonl').read_text().splitlines()]
assert [(r['pair'],r['variant']) for r in runs]==[(pair,v) for pair in range(4) for v in (['baseline','candidate'] if pair%2==0 else ['candidate','baseline'])]
expected=set(itertools.product([False,True],[1,16],range(8),[10,11]))
for run in runs:
 assert len(run['observations'])==64
 assert {(r['timers'],r['batch'],r['fixture'],r['iteration']) for r in run['observations']}==expected
summary=[]
for timers,batch in itertools.product([False,True],[1,16]):
 stats={}
 for variant in ['baseline','candidate']:
  rows=[r for run in runs if run['variant']==variant for r in run['observations'] if r['timers']==timers and r['batch']==batch];assert len(rows)==64
  stats[variant]={key:sorted({r[key] for r in rows}) for key in ['allocations','reallocations','requested_bytes']}
  assert all(len(value)==1 for value in stats[variant].values()),stats
 changes={key:stats['candidate'][key][0]-stats['baseline'][key][0] for key in stats['baseline']}
 assert changes['allocations']==-(batch-1)
 assert changes['reallocations']<=0 and changes['requested_bytes']<=0
 if batch==1:assert all(value==0 for value in changes.values())
 else:assert changes['reallocations']<0 and changes['requested_bytes']<0
 row=dict(timers=timers,batch=batch,samples_per_variant=64,**stats,changes=changes);summary.append(row);print(row)
(out/'allocation-analysis.json').write_text(json.dumps(summary,indent=2)+'\n')
