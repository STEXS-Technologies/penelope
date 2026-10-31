from pathlib import Path
import subprocess,os,json,hashlib
rows=[]
env={**os.environ,'TMPDIR':str(Path('target/qualification-temp').resolve())}
for compiler,directory in [('stable',Path('target/debug/deps')),('1.85.0',Path('target/worker-audit-msrv/debug/deps'))]:
 for kind,pattern,filter_text,repeats,passed in [('ports','process-*','claim_indexes::',25,5),('crash','penelope_sqlite-*','schema::tests::sigkill_at_schema_migration_boundaries_preserves_records_blobs_and_fences',10,1)]:
  candidates=sorted((p for p in directory.glob(pattern) if p.is_file() and p.stat().st_mode&0o111),key=lambda p:p.stat().st_mtime,reverse=True)
  binary=next(p for p in candidates if filter_text in subprocess.check_output([str(p),'--list'],text=True))
  for repeat in range(repeats):
   p=subprocess.run([str(binary),filter_text,'--test-threads=1'],env=env,text=True,capture_output=True,timeout=30,check=True)
   assert f'test result: ok. {passed} passed;' in p.stdout
   rows.append(dict(compiler=compiler,kind=kind,repeat=repeat,binary_sha256=hashlib.sha256(binary.read_bytes()).hexdigest(),stdout=p.stdout,stderr=p.stderr))
   Path('target/claim-indexes-repeated.json').write_text(json.dumps(rows,indent=2)+'\n')
print('Passed 70 isolated processes: 250 public-port migration/eligibility cases and 180 SIGKILL migration cuts')
