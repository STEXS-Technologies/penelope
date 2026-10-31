from pathlib import Path
import subprocess,json,time,platform
out=Path(__file__).resolve().parent
with (out/'runs.jsonl').open('w') as log:
 for pair in range(24):
  for variant in (['baseline','candidate'] if pair%2==0 else ['candidate','baseline']):
   for kind,bench in [('empty','lease_claim'),('batch','lease_batch')]:
    started=time.time();r=subprocess.run(['taskset','-c','0',str(out/(variant+'-'+bench))],text=True,capture_output=True,check=True)
    rows=[]
    for line in r.stdout.splitlines():
     fields=dict(part.split('=',1) for part in line.split());rows.append({k:(v=='true' if k=='timers' else int(v)) for k,v in fields.items()})
    assert len(rows)==(2 if kind=='empty' else 4)
    log.write(json.dumps(dict(pair=pair,variant=variant,kind=kind,started=started,rows=rows))+'\n');log.flush()
  print('pair',pair+1,flush=True)
(out/'environment.json').write_text(json.dumps(dict(platform=platform.platform(),cpu=subprocess.check_output(['lscpu'],text=True),rustc=subprocess.check_output(['rustc','-Vv'],text=True),cpu_affinity=0,pairs=24),indent=2)+'\n')
