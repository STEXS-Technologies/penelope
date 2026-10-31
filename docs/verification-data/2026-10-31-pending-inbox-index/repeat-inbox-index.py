from pathlib import Path
import subprocess,os,json
runs=[]
env={**os.environ,'RUSTC_WRAPPER':'','RUSTC_WORKSPACE_WRAPPER':'','TMPDIR':str(Path('target/qualification-temp').resolve())}
for compiler,toolchain,target in [('stable',None,None),('msrv','+1.85.0','target/worker-audit-msrv')]:
 build_env=env.copy()
 if target:build_env['CARGO_TARGET_DIR']=target
 for kind,name,selector,repetitions in [('port','process','inbox_index::',10),('migration','penelope_sqlite','schema::tests::',5)]:
  args=['cargo']+([toolchain] if toolchain else [])+['test','-p','penelope-sqlite']+(['--test','process'] if kind=='port' else ['--lib'])+['--no-run','--message-format=json']
  build=subprocess.run(args,env=build_env,text=True,capture_output=True,check=True)
  Path(f'target/inbox-index-repeat-build-{compiler}-{kind}.log').write_text(build.stdout+build.stderr)
  artifacts=[item for item in map(json.loads,build.stdout.splitlines()) if item.get('reason')=='compiler-artifact' and item.get('executable') and item['target']['name']==name];assert len(artifacts)==1
  binary=artifacts[0]['executable']
  for run in range(repetitions):
   result=subprocess.run([binary,selector,'--nocapture','--test-threads=1'],env=env,text=True,capture_output=True,timeout=60)
   output=result.stdout+result.stderr;Path(f'target/inbox-index-repeat-{compiler}-{kind}-{run}.log').write_text(output)
   assert result.returncode==0,output
   assert ('4 passed; 0 failed' if kind=='port' else '3 passed; 0 failed') in output
   if kind=='migration':assert output.count('signal=9 rows_preserved=true reopen_version=6')==18
   runs.append(dict(compiler=compiler,kind=kind,run=run,returncode=result.returncode,cases=4 if kind=='port' else 18))
Path('target/inbox-index-repeats.json').write_text(json.dumps(runs,indent=2)+'\n')
print('Passed 20 port processes (80 fixtures), 10 migration processes (180 SIGKILL cuts)')
