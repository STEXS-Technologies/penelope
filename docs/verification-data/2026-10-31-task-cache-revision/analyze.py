import json,pathlib,random,statistics
out=pathlib.Path(__file__).resolve().parent; rows=[json.loads(s) for s in (out/'runs.jsonl').read_text().splitlines()];assert len(rows)==144
result=[]
for external in [False,True]:
 for size in ([0,32,4096] if external else [0,32,4096,1048576]):
  for comparison,(before,after) in enumerate([('baseline','uncached'),('baseline','cached'),('uncached','cached')]):
   changes=[]; times={before:[],after:[]}
   for block in range(24):
    values={r['variant']:next(x['elapsed_ns']/128 for x in r['rows'] if x['size']==size) for r in rows if r['block']==block and r['external_commit']==external};assert len(values)==3
    changes.append(100*(values[after]/values[before]-1))
    for v in times:times[v].append(values[v])
   rng=random.Random(3102026+size+10000000*external+100000000*comparison); boot=sorted(statistics.mean(rng.choices(changes,k=24)) for _ in range(20000))
   result.append(dict(external_commit=external,size=size,before=before,after=after,blocks=24,before_ns=statistics.mean(times[before]),after_ns=statistics.mean(times[after]),change_percent=statistics.mean(changes),ci95=[boot[499],boot[19499]]))
(out/'analysis.json').write_text(json.dumps(result,indent=2)+'\n')
for r in result:print(f"external={r['external_commit']} size={r['size']} {r['before']}->{r['after']}: {r['change_percent']:.6f}% CI={r['ci95']} ns={r['before_ns']:.1f}->{r['after_ns']:.1f}")
