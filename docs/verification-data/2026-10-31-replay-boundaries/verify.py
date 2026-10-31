#!/usr/bin/env python3
"""Check retained qualification and fault-control evidence."""
import json
from pathlib import Path
root = Path(__file__).resolve().parent
runs = json.loads((root / "repeats.json").read_text())
assert len(runs) == 20
for toolchain in ("stable", "1.85.0"):
    selected = [r for r in runs if r["toolchain"] == toolchain]
    assert sorted(r["repeat"] for r in selected) == list(range(10))
    for run in selected:
        text = (root / run["log"]).read_text()
        assert run["exit"] == 0 and "1 passed; 0 failed" in text
for name in ("process.txt", "msrv.txt"):
    assert "61 passed; 0 failed" in (root / name).read_text()
control = (root / "checksum-control.txt").read_text()
assert "assertion `left == right` failed" in control
assert "left: Ok(OutcomeReplayPage" in control
assert "right: Err(Invariant)" in control
assert "0 passed; 1 failed" in control
assert "Finished" in (root / "clippy.txt").read_text()
print("Verified 20 repeated passes, both full process suites, Clippy and checksum-bypass rejection.")
