import json, os, subprocess
from pathlib import Path
root=Path.cwd()
name="worker::tests::panicking_waker_cannot_strand_accepted_work_or_discard_committed_result"
for toolchain in ["1.99.0", "1.85.0"]:
 env=dict(os.environ,RUSTC_WRAPPER="",RUSTC_WORKSPACE_WRAPPER="",TMPDIR=str(root/"target/qualification-temp"))
 if toolchain=="1.85.0": env["CARGO_TARGET_DIR"]=str(root/"target/worker-audit-msrv")
 built=subprocess.run(["cargo","+"+toolchain,"test","-p","penelope-sqlite","--lib","--no-run","--locked","--message-format=json"],env=env,text=True,capture_output=True,check=True)
 binaries=[json.loads(line)["executable"] for line in built.stdout.splitlines() if json.loads(line).get("reason")=="compiler-artifact" and json.loads(line).get("executable")]
 assert len(binaries)==1
 with (root/f"target/sqlite-wake-stress-{toolchain}.log").open("w") as log:
  log.write(built.stderr)
  for index in range(100):
   result=subprocess.run([binaries[0],name,"--exact","--quiet"],env=env,text=True,stdout=log,stderr=subprocess.STDOUT,timeout=20)
   if result.returncode: raise RuntimeError((toolchain,index,result.returncode))
  log.write(f"100 passed repetitions on Rust {toolchain}; each covers live admission and drain after close.\n")
 print(toolchain,"100 passed repetitions",flush=True)
