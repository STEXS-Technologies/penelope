import pathlib,subprocess,os
src=pathlib.Path('crates/penelope-sqlite/src/tasks.rs');original=src.read_bytes()
try:
 src.write_bytes(subprocess.check_output(['git','show','05cd3d0124b85d33a796281941850a8169cdbb98:crates/penelope-sqlite/src/tasks.rs']))
 env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(pathlib.Path.cwd()/'target/qualification-temp'))
 with open('target/task-replay-memo/baseline-control.log','w') as log:subprocess.run(['cargo','test','-p','penelope-sqlite','--test','tasks','task_replay_memo::','--','--nocapture'],stdout=log,stderr=subprocess.STDOUT,env=env,check=True)
 print('Unoptimized guarded baseline passes all nine new corruption fixtures.')
finally:src.write_bytes(original)
