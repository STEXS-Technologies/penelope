import pathlib,json,random,statistics
out=pathlib.Path(__file__).resolve().parent; runs=[json.loads(line) for line in (out/'runs.jsonl').read_text().splitlines()];assert len(runs)==288
analysis=[]
for kind in ['commits','reconstruction']:
 for changed in [False,True]:
  keys=([0,32,4096] if changed else [0,32,4096,1048576]) if kind=='commits' else [2,16,128]
  field='size' if kind=='commits' else 'history'
  for key in keys:
   changes=[]; times={'baseline':[],'candidate':[]}
   for pair in range(24):
    values={run['variant']:next(row['elapsed_ns']/row['iterations'] for row in run['rows'] if row[field]==key) for run in runs if run['pair']==pair and run['kind']==kind and run['changed']==changed};assert len(values)==2
    changes.append(100*(values['candidate']/values['baseline']-1))
    for variant in times:times[variant].append(values[variant])
   rng=random.Random(3102026+key+10000000*(kind=='reconstruction')+100000000*changed);boot=sorted(statistics.mean(rng.choices(changes,k=24)) for _ in range(20000))
   analysis.append(dict(kind=kind,changed=changed,key=key,pairs=24,baseline_ns=statistics.mean(times['baseline']),candidate_ns=statistics.mean(times['candidate']),change_percent=statistics.mean(changes),ci95=[boot[499],boot[19499]]))
allocations=[]
for changed in [False,True]:
 for history in [2,16,128]:
  for variant in ['baseline','candidate']:
   samples=[row for run in runs if run['kind']=='allocations' and run['changed']==changed and run['variant']==variant for row in run['rows'] if row['history']==history];assert len(samples)==384
   metrics=sorted(set((row['allocations'],row['reallocations'],row['requested_bytes']) for row in samples));assert len(metrics)==1
   allocations.append(dict(alternating=changed,history=history,variant=variant,samples=len(samples),metrics=metrics[0]))
(out/'analysis.json').write_text(json.dumps(dict(latency=analysis,allocations=allocations),indent=2)+'\n')
for r in analysis:print(r['kind'],r['changed'],r['key'],round(r['change_percent'],6),[round(x,6) for x in r['ci95']],round(r['baseline_ns'],2),round(r['candidate_ns'],2))
print('Allocations:',allocations)
