import importlib.util
from pathlib import Path
spec = importlib.util.spec_from_file_location("comparison", Path(__file__).resolve().parent.parent / "scripts/benchmark-decision-comparison.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
module.BENCHMARKS["penelope-executor"].append("definition_validation")
module.main()
