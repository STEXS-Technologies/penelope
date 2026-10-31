from pathlib import Path
import os,subprocess,json
root=Path.cwd();out=root/'target/payload-retry-fastpath/qualification';out.mkdir(exist_ok=True)
env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(root/'target/qualification-temp'),RUST_TEST_THREADS='1');env.pop('PENELOPE_TIMER_BATCH_CASE',None)
commands={
'workspace':['cargo','test','--workspace','--all-features','--tests','--locked','--exclude','penelope-fuzz'],
'msrv':['cargo','+1.85.0','test','-p','penelope-sqlite','-p','penelope-executor','-p','penelope-process-runtime','--all-features','--tests','--locked'],
'clippy':['cargo','clippy','--workspace','--all-targets','--all-features','--locked','--exclude','penelope-fuzz','--','-D','warnings'],
'msrv-check':['cargo','+1.85.0','check','-p','penelope-sqlite','-p','penelope-executor','--all-targets','--all-features','--locked'],
'fmt':['cargo','fmt','--all','--check'],
'layers':['bash','scripts/check-layer-boundaries.sh'],
'graph':['graphify','update','.']}
results={}
for name,command in commands.items():
 use=dict(env)
 if name.startswith('msrv'):use['CARGO_TARGET_DIR']='target/worker-audit-msrv'
 with (out/(name+'.txt')).open('w') as log:r=subprocess.run(command,env=use,stdout=log,stderr=subprocess.STDOUT)
 results[name]={'command':command,'exit_code':r.returncode}
 (out/'results.json').write_text(json.dumps(results,indent=2)+'\n')
 if r.returncode:raise SystemExit(name+' failed')
