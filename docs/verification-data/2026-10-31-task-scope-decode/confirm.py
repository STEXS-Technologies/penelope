import subprocess,pathlib,json,time,platform,itertools,os
out=pathlib.Path(__file__).resolve().parent; orders=list(itertools.permutations(['pre_guard','baseline','candidate']))
with (out/'confirm-runs.jsonl').open('w') as log:
 for block in range(24):
  for external in ([False,True] if block%2==0 else [True,False]):
   for variant in orders[block%6]:
    env=dict(os.environ);env.pop('PENELOPE_BENCH_EXTERNAL_COMMIT',None)
    if external:env['PENELOPE_BENCH_EXTERNAL_COMMIT']='1'
    started=time.time();p=subprocess.run(['taskset','-c','0',str(out/variant)],text=True,capture_output=True,env=env,check=True)
    rows=[json.loads(line) for line in p.stdout.splitlines()];assert len(rows)==(3 if external else 4)
    assert all(r['external_commit']==external for r in rows)
    log.write(json.dumps(dict(block=block,variant=variant,external_commit=external,started=started,rows=rows))+'\n');log.flush()
  print('block',block+1,flush=True)
(out/'confirm-environment.json').write_text(json.dumps(dict(platform=platform.platform(),cpu=subprocess.check_output(['lscpu'],text=True),rustc=subprocess.check_output(['rustc','-Vv'],text=True),blocks=24,cpu_affinity=0),indent=2)+'\n')
