from pathlib import Path
import subprocess,os,re,json,time
runs=[]
env={**os.environ,'RUSTC_WRAPPER':'','RUSTC_WORKSPACE_WRAPPER':'','TMPDIR':str(Path('target/qualification-temp').resolve())}
for compiler,toolchain,target in [('stable',None,None),('msrv','+1.85.0','target/worker-audit-msrv')]:
 build_env=env.copy()
 if target:build_env['CARGO_TARGET_DIR']=target
 args=['cargo']+([toolchain] if toolchain else [])+['test','-p','penelope-sqlite','--test','process','--no-run','--message-format=json']
 build=subprocess.run(args,env=build_env,text=True,capture_output=True,check=True)
 Path(f'target/replay-cache-repeat-build-{compiler}.log').write_text(build.stdout+build.stderr)
 artifacts=[item for item in map(json.loads,build.stdout.splitlines()) if item.get('reason')=='compiler-artifact' and item.get('executable') and item['target']['name']=='process'];assert len(artifacts)==1
 binary=artifacts[0]['executable']
 for run in range(10):
  result=subprocess.run([binary,'replay_cache::','--nocapture','--test-threads=1'],env=env,text=True,capture_output=True,timeout=30)
  output=result.stdout+result.stderr;Path(f'target/replay-cache-repeat-{compiler}-{run}.log').write_text(output)
  assert result.returncode==0 and '1 passed; 0 failed' in output,output
  runs.append(dict(compiler=compiler,run=run,returncode=result.returncode))
Path('target/replay-cache-repeats.json').write_text(json.dumps(runs,indent=2)+'\n')
print('Passed 20 isolated warm-cache regression processes')
