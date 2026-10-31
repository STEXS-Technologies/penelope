import subprocess,pathlib,os
src=pathlib.Path('crates/penelope-sqlite/src/tasks.rs'); original=src.read_bytes(); old=b'.filter(|blob| Some(*blob) != verified_blob)'; new=b'.filter(|blob| verified_blob.map(|verified: BlobRef| verified.digest) != Some(blob.digest))'; assert old in original
try:
 src.write_bytes(original.replace(old,new))
 env=dict(os.environ,RUSTC_WRAPPER='',RUSTC_WORKSPACE_WRAPPER='',TMPDIR=str(pathlib.Path.cwd()/'target/qualification-temp'))
 with open('target/task-replay-memo/negative-control.log','w') as log:
  process=subprocess.run(['cargo','test','-p','penelope-sqlite','--test','tasks','task_replay_memo::','--','--nocapture'],stdout=log,stderr=subprocess.STDOUT,env=env)
 assert process.returncode==101
 log=pathlib.Path('target/task-replay-memo/negative-control.log').read_text();assert 'append length-two' in log and 'test result: FAILED' in log
 print('Digest-only negative control failed as expected on changed byte count.')
finally:src.write_bytes(original)
