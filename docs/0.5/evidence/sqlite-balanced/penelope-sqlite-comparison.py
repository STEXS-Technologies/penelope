import json,os,subprocess
from pathlib import Path
root=Path('/home/ac/projects/penelope'); baseline=Path('/tmp/penelope-v040-baseline')
out=Path('/tmp/penelope-050-sqlite-balanced-performance');out.mkdir(exist_ok=False)
assert not subprocess.check_output(['git','diff','HEAD','--'],cwd=root)
assert not subprocess.check_output(['git','diff','HEAD','--'],cwd=baseline)
source='crates/penelope-sqlite/benches/durable_store.rs'
assert (root/source).read_bytes()==(baseline/source).read_bytes()
os.sched_setaffinity(0,{4,5,6,7})
plan={'rounds':9,'warmup_rounds':1,'order':'alternating before/after by round; full warmup pair retained but excluded from aggregate','revisions':{label:subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip() for label,repo in [('before',baseline),('after',root)]},'concurrent_work':'Full unchanged chaos campaign pinned to separate CPUs 8-15; unrelated shared host activity and caches uncontrolled.','data_filesystem':'Same Btrfs filesystem on NVMe; WAL/FULL, no cache eviction or fsync bypass.'}
(out/'comparison.json').write_text(json.dumps(plan,indent=2)+'\n')
for round_number in range(10):
 labels=['before','after'] if round_number%2==0 else ['after','before']
 for label in labels:
  repo=baseline if label=='before' else root
  name=('warmup' if round_number==0 else f'round-{round_number}')+'-'+label
  print('Running '+name,flush=True)
  with (out/(name+'.log')).open('w') as log:
   subprocess.run(['python3',str(root/'scripts/benchmark-durable-store.py'),str(out/name),'--repository',str(repo),'--data-parent',str(root/'target/perf-data'),'--cpu','4','--runs','1','--toolchain','1.100.0'],env={**os.environ,'RUSTC_WRAPPER':'','RUSTC_WORKSPACE_WRAPPER':'','CARGO_BUILD_JOBS':'2','CARGO_TARGET_DIR':str(root/f'target/sqlite-performance-{label}')},stdout=log,stderr=subprocess.STDOUT,check=True)
 print(f'Completed pair {round_number}',flush=True)
