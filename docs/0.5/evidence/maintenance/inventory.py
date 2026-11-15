#!/usr/bin/env python3
"""Count tracked crate Rust text and declared dependency edges at exact revisions."""
import hashlib
import io
import json
from pathlib import Path
import subprocess
import tarfile
import tomllib


def inventory(revision):
    commit = subprocess.check_output(
        ['git', 'rev-parse', '--verify', '--end-of-options', revision + '^{commit}'], text=True
    ).strip()
    data = subprocess.check_output(['git', 'archive', commit])
    with tarfile.open(fileobj=io.BytesIO(data)) as archive:
        files = {m.name: archive.extractfile(m).read() for m in archive if m.isfile()}
        aliases = {m.name: m.linkname for m in archive if m.issym()}
    workspace = tomllib.loads(files['Cargo.toml'].decode())['workspace']
    crates = {}
    for member in workspace['members']:
        if not member.startswith('crates/'):
            continue
        manifest = tomllib.loads(files[member + '/Cargo.toml'].decode())
        package = manifest['package']['name']
        counts = {}
        for path, content in files.items():
            if not path.startswith(member + '/') or not path.endswith('.rs'):
                continue
            part = path[len(member) + 1:].split('/')[0]
            if part not in ('src', 'tests', 'benches', 'examples'):
                continue
            count = counts.setdefault(part, {'files': 0, 'physical_lines': 0, 'nonblank_lines': 0})
            lines = content.splitlines()
            count['files'] += 1
            count['physical_lines'] += len(lines)
            count['nonblank_lines'] += sum(bool(line.strip()) for line in lines)
        edges = []
        for condition, scope in [('all', manifest), *manifest.get('target', {}).items()]:
            for kind in ('dependencies', 'dev-dependencies', 'build-dependencies'):
                for name, declaration in scope.get(kind, {}).items():
                    declaration = {'version': declaration} if isinstance(declaration, str) else declaration
                    inherited = workspace.get('dependencies', {}).get(name, {}) if declaration.get('workspace') else {}
                    inherited = {'version': inherited} if isinstance(inherited, str) else inherited
                    resolved = dict(inherited, **declaration)
                    edges.append({'name': resolved.get('package', name), 'kind': kind,
                                  'target': condition, 'optional': resolved.get('optional', False)})
        crates[package] = {'rust_text': counts, 'declared_edges': sorted(edges, key=json.dumps)}
    totals = {}
    for crate in crates.values():
        for part, counts in crate['rust_text'].items():
            total = totals.setdefault(part, dict.fromkeys(counts, 0))
            for key, value in counts.items():
                total[key] += value
    return {'commit': commit, 'crate_count': len(crates), 'totals': totals, 'crates': crates,
            'source_files': {p: len(content.splitlines()) for p, content in files.items()
                             if p.startswith('crates/') and '/src/' in p and p.endswith('.rs')},
            'fixture_aliases': {p: v for p, v in aliases.items() if p.startswith('crates/')},
            'tracked_markdown_files': sum(p.endswith('.md') for p in files),
            'workspace_manifest_sha256': hashlib.sha256(files['Cargo.toml']).hexdigest(),
            'lock_sha256': hashlib.sha256(files['Cargo.lock']).hexdigest()}


if __name__ == '__main__':
    import argparse
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--before', required=True)
    parser.add_argument('--after', default='HEAD')
    args = parser.parse_args()
    before, after = inventory(args.before), inventory(args.after)
    common = before['source_files'].keys() & after['source_files'].keys()
    print(json.dumps({'before': before, 'after': after,
        'source_text_change': {
            'existing_path_line_delta': sum(after['source_files'][p] - before['source_files'][p] for p in common),
            'added_path_lines': sum(n for p, n in after['source_files'].items() if p not in before['source_files']),
            'removed_path_lines': sum(n for p, n in before['source_files'].items() if p not in after['source_files'])},
        'method': 'Tracked regular Rust files by directory; physical and nonblank text lines, including comments and inline/cfg test code. Symlink aliases are counted separately, never as independent source implementations. Declared edges include optional and target-conditioned dependencies; not a resolved feature-specific dependency graph.',
        'limits': 'Source-directory counts are not production-only semantic LoC, complexity or duplication metrics. Counts alone do not prove maintainability. Fuzz, CI fixtures, scripts and historical evidence are outside crate Rust totals.'}, indent=2))
