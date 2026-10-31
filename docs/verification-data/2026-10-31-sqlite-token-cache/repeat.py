from pathlib import Path
import subprocess,os,json,hashlib
name='lease_contracts::warmed_token_statements_preserve_batch_rollback_retry_and_reopen'
env={**os.environ,'TMPDIR':str(Path('target/qualification-temp').resolve())}
rows=[]
for compiler,directory in [('stable',Path('target/debug/deps')),('1.85.0',Path('target/worker-audit-msrv/debug/deps'))]:
 candidates=sorted((p for p in directory.glob('process-*') if p.is_file() and p.stat().st_mode&0o111),key=lambda p:p.stat().st_mtime,reverse=True)
 binary=next(p for p in candidates if name in subprocess.check_output([str(p),'--list'],text=True))
 for repeat in range(25):
  p=subprocess.run([str(binary),'--exact',name,'--nocapture'],env=env,text=True,capture_output=True,check=True,timeout=30)
  assert '1 passed; 0 failed' in p.stdout
  rows.append(dict(compiler=compiler,repeat=repeat,binary=str(binary),binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),stdout=p.stdout,stderr=p.stderr))
Path('target/token-cache-repeated.json').write_text(json.dumps(rows,indent=2)+'\n')
print('Passed',len(rows),'isolated processes /',len(rows)*2,'outbox and timer rollback/retry/reopen cases')
