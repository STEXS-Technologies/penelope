import pathlib,json,random,statistics,itertools
out=pathlib.Path(__file__).resolve().parent;runs=[json.loads(line) for line in (out/'runs.jsonl').read_text().splitlines()];assert len(runs)==480
configs=[('commits',e,False,False) for e in [False,True]]+[('reconstruction',False,a,p) for p,a in itertools.product([False,True],repeat=2)]
latency=[]
for kind,external,alternating,reformatted in configs:
 keys=([0,32,4096] if external else [0,32,4096,1048576]) if kind=='commits' else [2,16,128];field='size' if kind=='commits' else 'history'
 for key in keys:
  changes=[];times={'baseline':[],'candidate':[]}
  for pair in range(24):
   values={r['variant']:next(x['elapsed_ns']/x['iterations'] for x in r['rows'] if x[field]==key) for r in runs if r['pair']==pair and (r['kind'],r['external'],r['alternating'],r['reformatted'])==(kind,external,alternating,reformatted)};assert len(values)==2
   changes.append(100*(values['candidate']/values['baseline']-1))
   for variant in times:times[variant].append(values[variant])
  rng=random.Random(3102026+key+10000000*(kind=='reconstruction')+100000000*external+1000000000*alternating+10000000000*reformatted);boot=sorted(statistics.mean(rng.choices(changes,k=24)) for _ in range(20000))
  latency.append(dict(kind=kind,external=external,alternating=alternating,reformatted=reformatted,key=key,pairs=24,baseline_ns=statistics.mean(times['baseline']),candidate_ns=statistics.mean(times['candidate']),change_percent=statistics.mean(changes),ci95=[boot[499],boot[19499]]))
allocations=[]
for pretty,alt in itertools.product([False,True],repeat=2):
 for history in [2,16,128]:
  for variant in ['baseline','candidate']:
   samples=[row for r in runs if (r['kind'],r['alternating'],r['reformatted'],r['variant'])==('allocations',alt,pretty,variant) for row in r['rows'] if row['history']==history];assert len(samples)==384
   metrics=sorted(set((r['allocations'],r['reallocations'],r['requested_bytes']) for r in samples));assert len(metrics)==1
   allocations.append(dict(alternating=alt,reformatted=pretty,history=history,variant=variant,samples=384,metrics=metrics[0]))
(out/'analysis.json').write_text(json.dumps(dict(latency=latency,allocations=allocations),indent=2)+'\n')
for r in latency:print(r['kind'],'external',r['external'],'alt',r['alternating'],'pretty',r['reformatted'],r['key'],round(r['change_percent'],5),[round(x,5) for x in r['ci95']],round(r['baseline_ns'],1),round(r['candidate_ns'],1))
print('Allocation profiles:',allocations)
