#!/usr/bin/env python3
"""Build and run paired end-to-end synthetic workflows; preserve every sample."""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
from application_workloads_summary import CASES, summarize

ROOT = Path(__file__).resolve().parents[1]

def integers(value):
    return [int(item) for item in value.split(",")]

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "target" / ("application-workloads-"+datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%SZ")))
    parser.add_argument("--repeats", type=int, default=4)
    parser.add_argument("--rounds", type=int, default=8)
    parser.add_argument("--iterations", type=int, default=4)
    parser.add_argument("--work", type=integers, default=[4096,65536,1048576])
    parser.add_argument("--workers", type=integers, default=[1,4])
    parser.add_argument("--mix", type=Path, help="JSON case weights; default equal workflow counts across all six cases")
    args = parser.parse_args()
    if not (2 <= args.repeats <= 32 and 2 <= args.rounds <= 64 and args.rounds % 2 == 0 and 1 <= args.iterations <= 1024):
        parser.error("need 2..32 repeats, even 2..64 rounds and 1..1024 iterations")
    if not args.work or any(not 0 <= n <= 16777216 for n in args.work) or len(set(args.work)) != len(args.work):
        parser.error("work values must be distinct and within 0..16777216")
    available = sorted(os.sched_getaffinity(0)) if hasattr(os, "sched_getaffinity") else list(range(os.cpu_count() or 1))
    if not args.workers or any(not 1 <= n <= min(32,len(available)) for n in args.workers) or len(set(args.workers)) != len(args.workers):
        parser.error("worker counts must be distinct, positive and fit available CPUs (maximum 32)")
    mix = json.loads(args.mix.read_text()) if args.mix else dict.fromkeys(CASES, 1)
    import math
    if set(mix) != set(CASES) or not all(isinstance(v,(int,float)) and math.isfinite(v) and v>=0 for v in mix.values()) or sum(mix.values())<=0:
        parser.error("invalid mix: provide finite nonnegative weights for all six cases, with positive total")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    (output / "databases").mkdir()
    env = dict(os.environ,RUSTC_WRAPPER="",RUSTC_WORKSPACE_WRAPPER="",TMPDIR=str(output / "databases"))
    command = ["cargo","build","--release","--locked","-p","penelope-executor","--bench","application_workloads","--message-format=json"]
    with (output/"build.txt").open("w") as log:
        built = subprocess.run(command,cwd=ROOT,env=env,stdout=subprocess.PIPE,stderr=log,text=True,check=True)
    binary = next(Path(item["executable"]) for line in built.stdout.splitlines() if (item:=json.loads(line)).get("executable") and item.get("target",{}).get("name")=="application_workloads")
    sources = [ROOT/"crates/penelope-executor/benches/application_workloads.rs",*sorted((ROOT/"crates/penelope-executor/benches/support/application_workloads").glob("*.rs")),Path(__file__),ROOT/"scripts/application_workloads_summary.py",ROOT/"Cargo.lock"]
    metadata = {"schema":1,"git_head":subprocess.check_output(["git","rev-parse","HEAD"],cwd=ROOT,text=True).strip(),"git_status":subprocess.check_output(["git","status","--short"],cwd=ROOT,text=True),"rustc":subprocess.check_output(["rustc","--version"],text=True).strip(),"platform":platform.platform(),"repeats":args.repeats,"rounds":args.rounds,"iterations":args.iterations,"work":args.work,"workers":args.workers,"mix":mix,"cpu_affinity":{str(n):available[:n] for n in args.workers},"database_root":str(output/"databases"),"binary_sha256":hashlib.sha256(binary.read_bytes()).hexdigest(),"source_sha256":{str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sources},"scope":"synthetic business work versus additional orchestration/reliability services; not every Penelope path or a consuming application"}
    if platform.system()=="Linux":
        metadata["filesystem"] = subprocess.check_output(["findmnt","-T",str(output),"-n","-o","FSTYPE,SOURCE"],text=True).strip()
    (output/"metadata.json").write_text(json.dumps(metadata,indent=2)+"\n")
    rows=[]
    # No builds, tests, allocation probes or summarization overlap timing.
    with (output/"samples.jsonl").open("w") as log:
        for repeat in range(args.repeats):
            configs=[(workers,work) for workers in args.workers for work in args.work]
            if repeat%2: configs.reverse()
            for workers,work in configs:
                command=[str(binary),"--workers",str(workers),"--work",str(work),"--rounds",str(args.rounds),"--iterations",str(args.iterations)]
                if platform.system()=="Linux":command=["taskset","-c",",".join(map(str,available[:workers])),*command]
                result=subprocess.run(command,cwd=ROOT,env=env,stdout=subprocess.PIPE,text=True,check=True)
                samples=[json.loads(line) for line in result.stdout.splitlines()]
                if len(samples)!=len(CASES)*args.rounds:raise ValueError("incomplete binary output")
                for row in samples:
                    row["repeat"]=repeat
                    rows.append(row)
                    log.write(json.dumps(row)+"\n")
                log.flush()
                print(f"repeat={repeat} workers={workers} work={work} complete",flush=True)
    summary=summarize(rows,metadata)
    (output/"summary.json").write_text(json.dumps(summary,indent=2)+"\n")
    print(f"Verified {len(rows)} paired samples. Results: {output}")

if __name__=="__main__":main()
