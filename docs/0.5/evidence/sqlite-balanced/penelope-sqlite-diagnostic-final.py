import json,os,subprocess
from pathlib import Path
root=Path('/home/ac/projects/penelope');out=Path('/tmp/penelope-050-sqlite-diagnostic-final');out.mkdir(exist_ok=False)
repos={'baseline':Path('/tmp/penelope-v040-baseline'),'patched-baseline':Path('/tmp/penelope-v040-patched-deps'),'candidate':root}
os.sched_setaffinity(0,{4,5,6,7})
(out/'diagnostic.json').write_text(json.dumps({'purpose':'Attribute legacy SQLite replay regression without changing library sources. Patched baseline is original source plus only current Cargo.lock, all workspace SQLite dependency requirements, and approved workspace MSRV; lockfile workspace edges refreshed offline with no external version/checksum changes. All variants use identical benchmark.','rounds':9,'warmup_rounds':1,'order':'Rotate three variants each round; warmup retained separately.','revisions':{label:subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip() for label,repo in repos.items()},'patched_baseline_diff':subprocess.check_output(['git','diff','HEAD'],cwd=repos['patched-baseline'],text=True)},indent=2)+'\n')
for n in range(10):
 labels=list(repos);labels=labels[n%3:]+labels[:n%3]
 for label in labels:
  name=('warmup' if n==0 else f'round-{n}')+'-'+label
  print('Running '+name,flush=True)
  target={'baseline':'before','candidate':'after','patched-baseline':'patched-baseline'}[label]
  with (out/(name+'.log')).open('w') as log:
   subprocess.run(['python3',str(root/'scripts/benchmark-durable-store.py'),str(out/name),'--repository',str(repos[label]),'--data-parent',str(root/'target/perf-data'),'--cpu','4','--runs','1','--toolchain','1.100.0'],env={**os.environ,'RUSTC_WRAPPER':'','RUSTC_WORKSPACE_WRAPPER':'','CARGO_BUILD_JOBS':'2','CARGO_TARGET_DIR':str(root/f'target/sqlite-performance-{target}')},stdout=log,stderr=subprocess.STDOUT,check=True)
 print(f'Completed round {n}',flush=True)
