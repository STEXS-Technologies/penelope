"""Verify archived frontier regression evidence and pinned source hashes."""
import gzip
import hashlib
import json
from pathlib import Path
import re
import subprocess

root = Path(__file__).resolve().parent
metadata = json.loads((root / "metadata.json").read_text())
for path, expected in metadata["source_sha256"].items():
    data = subprocess.check_output(["git", "show", metadata["candidate"] + ":" + path])
    assert hashlib.sha256(data).hexdigest() == expected, path

def log(name):
    return gzip.decompress((root / (name + ".gz")).read_bytes()).decode()

baseline = re.findall(r"damage=([^\n]+)", log("process-frontier-baseline.log"))
assert len(baseline) == 36
assert sum("result=Ok(())" in row for row in baseline) == 20
assert sum("result=Err(Conflict)" in row for row in baseline) == 4
assert sum("unchanged=false" in row for row in baseline) == 2
runs = json.loads((root / "repeats.json").read_text())
assert len(runs) == 20
for run in runs:
    output = log(f"frontier-repeat-{run['compiler']}-{run['run']}.log")
    assert output.count("result=Err(Invariant) unchanged=true") == 36
    assert "3 passed; 0 failed" in output
    for count in (1, 128, 4096):
        assert f"sqlite=3.53.2 history={count} unrelated=8192 vm_steps=37 sorts=0" in output
for name in ("frontier-process-final.log", "frontier-process-final-msrv.log"):
    assert "43 passed; 0 failed" in log(name)
assert "Finished" in log("frontier-clippy.log")
assert "error:" not in log("frontier-clippy.log")
assert "test result: FAILED" not in log("frontier-workspace.log")
print("Verified pinned sources, baseline reproduction, 720 fault cases, and indexed query work.")
