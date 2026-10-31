import pathlib
out = pathlib.Path(__file__).resolve().parent
for target, source in [('pre_guard','pre_guard'), ('guarded','baseline'), ('optimized','candidate')]:
    path = out / target
    path.write_bytes((out / (source + '-task_blob_commits')).read_bytes())
    path.chmod(0o755)
