import subprocess,pathlib,json,time,platform
out=pathlib.Path(__file__).resolve().parent
with (out/'runs.jsonl').open('w') as log:
 for pair in range(24):
  order=['baseline','candidate'] if pair%2==0 else ['candidate','baseline']
  for variant in order:
   started=time.time(); p=subprocess.run(['taskset','-c','0',str(out/variant)],text=True,capture_output=True,check=True)
   rows=[json.loads(s) for s in p.stdout.splitlines()];assert len(rows)==4
   log.write(json.dumps(dict(pair=pair,variant=variant,started=started,rows=rows))+'\n');log.flush()
  print('pair',pair+1,flush=True)
(out/'environment.json').write_text(json.dumps(dict(platform=platform.platform(),cpu=subprocess.check_output(['lscpu'],text=True),rustc=subprocess.check_output(['rustc','-Vv'],text=True),pairs=24,cpu_affinity=0),indent=2)+'\n')
