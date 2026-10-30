"""Reproduce summary.json from the 40 retained process records."""
import importlib.util
import json
import sys
sys.dont_write_bytecode = True
from pathlib import Path
root = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("stats", root / "summarize-decision-perf.py")
stats = importlib.util.module_from_spec(spec)
spec.loader.exec_module(stats)
metadata = json.loads((root / "metadata.json").read_text())
records = {label: [json.loads((root / f"run-{i}-{label}-worker_protocol.json").read_text())
                   for i in range(1, 21)] for label in ["before", "after"]}
result = {"revisions": metadata["revisions"], "operations": {}, "whole_process_counters": {}}
for index in range(4):
    name = records["before"][0]["observations"][index]["operation"]
    result["operations"][name] = stats.paired_statistics(*[
        [record["observations"][index]["ns_per_operation"] for record in records[label]]
        for label in records])
for event in ["instructions", "cycles", "task-clock"]:
    result["whole_process_counters"][event] = stats.paired_statistics(*[
        [float(next(counter["counter-value"] for counter in record["perf"]
                    if counter["event"] == event)) for record in records[label]]
        for label in records])
print(json.dumps(result, indent=2))
