from pathlib import Path
import subprocess,os
paths=[Path('crates/penelope-sqlite/src/process.rs'),Path('crates/penelope-sqlite/src/tasks.rs')]
current={path:path.read_bytes() for path in paths}
try:
 for path in paths:path.write_bytes(subprocess.check_output(['git','show','ec4399f0cc27698a63b7c9ac23e408b0b6557785:'+str(path)]))
 env={**os.environ,'RUSTC_WRAPPER':'','RUSTC_WORKSPACE_WRAPPER':'','TMPDIR':str(Path('target/qualification-temp').resolve())}
 result=subprocess.run(['cargo','test','-p','penelope-sqlite','--test','process','replay_columns::','--','--nocapture','--test-threads=1'],env=env,text=True,capture_output=True)
 Path('target/replay-borrow-baseline-columns.log').write_text(result.stdout+result.stderr);result.check_returncode()
finally:
 for path in paths:path.write_bytes(current[path])
print('Baseline passes the same 20 column validation cases')
