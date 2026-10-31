import json,pathlib,random,statistics
out=pathlib.Path(__file__).resolve().parent
rows=[json.loads(s) for s in (out/'runs.jsonl').read_text().splitlines()]; assert len(rows)==48
result=[]
for size in [0,32,4096,1048576]:
 pairs=[]; means={v:[] for v in ['baseline','candidate']}
 for pair in range(24):
  values={r['variant']:next(x['elapsed_ns']/x['iterations'] for x in r['rows'] if x['size']==size) for r in rows if r['pair']==pair};assert len(values)==2
  pairs.append(100*(values['candidate']/values['baseline']-1))
  for v in means:means[v].append(values[v])
 rng=random.Random(3102026+size); boot=sorted(statistics.mean(rng.choices(pairs,k=24)) for _ in range(20000))
 result.append(dict(size=size,pairs=24,baseline_ns=statistics.mean(means['baseline']),candidate_ns=statistics.mean(means['candidate']),change_percent=statistics.mean(pairs),ci95=[boot[499],boot[19499]]))
(out/'analysis.json').write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
