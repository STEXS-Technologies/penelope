"""One-off source audit; requires tree-sitter==0.25.2, tree-sitter-rust==0.24.2.

Requires Python 3.13; these are audit-tool dependencies, not library dependencies.
Counts the union of potentially production Rust source across cfg/platforms.
Excludes comments and items provably disabled when test=false, including external
module files. Macro definitions are counted unexpanded. This is neither compiled
code size nor a semantic duplication score. Exact whole-function body clones are
review candidates, never instructions to merge distinct authority boundaries.
Code lines are the union of physical lines covered by non-comment leaf tokens.
Unknown cfg predicates and unexpanded macro tokens are conservatively included.
The clone check compares complete function bodies without renaming identifiers;
it cannot identify semantic, partial, renamed or macro-expanded duplication.
"""
import argparse
import hashlib
import io
import json
from pathlib import PurePosixPath
import re
import subprocess
import sys
import tarfile
from importlib.metadata import version
import tree_sitter
import tree_sitter_rust


def cfg(expression):
    """Three-valued cfg evaluation: unknown platform/feature predicates stay live."""
    expression = expression.strip()
    if expression == 'test':
        return False
    match = re.fullmatch(r'(all|any|not)\((.*)\)', expression, re.S)
    if not match:
        return None
    depth = 0
    parts = []
    start = 0
    quoted = False
    escaped = False
    for i, character in enumerate(match[2]):
        if quoted:
            if escaped: escaped = False
            elif character == '\\': escaped = True
            elif character == '"': quoted = False
        elif character == '"': quoted = True
        elif character == '(': depth += 1
        elif character == ')': depth -= 1
        elif character == ',' and depth == 0:
            parts.append(match[2][start:i]); start = i + 1
    parts.append(match[2][start:])
    values = [cfg(part) for part in parts if part.strip()]
    if match[1] == 'not':
        assert len(values) == 1
        return None if values[0] is None else not values[0]
    if match[1] == 'all':
        return False if False in values else True if all(v is True for v in values) else None
    return True if True in values else False if all(v is False for v in values) else None


def disabled(attributes):
    for attribute in attributes:
        text = attribute.text.decode()
        if re.fullmatch(r'#\[\s*test\s*\]', text):
            return True
        match = re.fullmatch(r'#\[\s*cfg\s*\((.*)\)\s*\]', text, re.S)
        if match and cfg(match[1]) is False:
            return True
    return False


def inventory(revision):
    commit = subprocess.check_output(['git', 'rev-parse', '--verify', '--end-of-options', revision + '^{commit}'], text=True).strip()
    archive = subprocess.check_output(['git', 'archive', commit])
    with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
        files = {m.name: tar.extractfile(m).read() for m in tar
                 if m.isfile() and m.name.startswith('crates/') and '/src/' in m.name and m.name.endswith('.rs')}
    parser = tree_sitter.Parser(tree_sitter.Language(tree_sitter_rust.language()))
    trees = {path: parser.parse(source) for path, source in files.items()}
    errors = [path for path, tree in trees.items() if tree.root_node.has_error]
    assert not errors, errors
    edges = {path: set() for path in files}
    live_edges = {path: set() for path in files}
    test_roots = set()

    def references(path, node, attributes):
        name = node.child_by_field_name('name').text.decode()
        explicit = next((re.search(r'#\[path\s*=\s*"([^"]+)"\]', a.text.decode())
                         for a in attributes if a.text.decode().startswith('#[path')), None)
        parent = PurePosixPath(path).parent
        if explicit:
            candidates = [str(parent / explicit[1])]
        else:
            module_dir = parent if PurePosixPath(path).name in ['lib.rs', 'mod.rs', 'main.rs'] else parent / PurePosixPath(path).stem
            candidates = [str(module_dir / (name + '.rs')), str(module_dir / name / 'mod.rs')]
        return {candidate for candidate in candidates if candidate in files}

    def visit(path, node, inherited=False, tokens=None, functions=None):
        attributes = []
        for child in node.children:
            if child.type in ['line_comment', 'block_comment']:
                continue
            if child.type == 'attribute_item':
                attributes.append(child)
                continue
            skip = inherited or disabled(attributes)
            if child.type == 'mod_item' and child.child_by_field_name('body') is None:
                targets = references(path, child, attributes)
                edges[path].update(targets)
                if skip:
                    test_roots.update(targets)
                else:
                    live_edges[path].update(targets)
            if not skip and tokens is not None:
                for attribute in attributes:
                    leaves(attribute, tokens)
            attributes = []
            if skip:
                visit(path, child, True)
            elif child.child_count:
                if functions is not None and child.type == 'function_item':
                    body = child.child_by_field_name('body')
                    body_tokens = []
                    visit(path, body, tokens=body_tokens)
                    if len(body_tokens) >= 40:
                        normalized = b' '.join(t.text for t in body_tokens)
                        functions.append({'path': path, 'line': child.start_point.row + 1,
                            'name': child.child_by_field_name('name').text.decode(),
                            'tokens': len(body_tokens), 'body_sha256': hashlib.sha256(normalized).hexdigest()})
                visit(path, child, tokens=tokens, functions=functions)
            elif tokens is not None:
                tokens.append(child)

    def leaves(node, tokens):
        if node.type in ['line_comment', 'block_comment']:
            return
        if not node.child_count:
            tokens.append(node)
        for child in node.children:
            leaves(child, tokens)

    for path, tree in trees.items():
        visit(path, tree.root_node)
    def reachable(roots, adjacency):
        seen, pending = set(roots), list(roots)
        while pending:
            for target in adjacency[pending.pop()] - seen:
                seen.add(target)
                pending.append(target)
        return seen

    # Production reachability protects files also used outside tests. Propagate
    # through nested external test modules, without letting their own imports
    # make those nested files appear production-reachable.
    roots = {p for p in files if PurePosixPath(p).name in ['lib.rs', 'main.rs']}
    live = reachable(roots, live_edges)
    excluded = reachable(test_roots, edges) - live
    records = {}
    functions = []
    for path, tree in trees.items():
        if path in excluded:
            continue
        tokens = []
        visit(path, tree.root_node, tokens=tokens, functions=functions)
        lines = set()
        for token in tokens:
            lines.update(range(token.start_point.row + 1, token.end_point.row + 2))
        records[path] = {'code_lines': len(lines), 'lexical_tokens': len(tokens),
                         'source_sha256': hashlib.sha256(files[path]).hexdigest()}
    groups = {}
    for function in functions:
        groups.setdefault(function['body_sha256'], []).append(function)
    clones = [members for members in groups.values() if len(members) > 1]
    return {'commit': commit, 'source_files': records, 'excluded_test_files': sorted(excluded),
            'potential_production_code_lines': sum(v['code_lines'] for v in records.values()),
            'lexical_tokens': sum(v['lexical_tokens'] for v in records.values()),
            'exact_function_body_clone_candidates': clones,
            'conservatively_included_unreachable_files': sorted(set(files) - excluded - live)}


if __name__ == '__main__':
    cli = argparse.ArgumentParser(description=__doc__)
    cli.add_argument('--before', required=True)
    cli.add_argument('--after', required=True)
    args = cli.parse_args()
    if sys.version_info[:2] != (3, 13):
        cli.error('Use Python 3.13 for this pinned source audit.')
    required = {'tree-sitter': '0.25.2', 'tree-sitter-rust': '0.24.2'}
    if any(version(name) != expected for name, expected in required.items()):
        cli.error('Use tree-sitter==0.25.2 and tree-sitter-rust==0.24.2.')
    # Boundary checks for three-valued test cfg filtering; no feature guesswork.
    assert cfg('all(test, unix)') is False
    assert cfg('not(test)') is True
    assert cfg('any(test, feature = "test,quoted")') is None
    assert cfg('not(any(test, feature = "x"))') is None
    before, after = inventory(args.before), inventory(args.after)
    common = before['source_files'].keys() & after['source_files'].keys()
    delta = sum(after['source_files'][p]['code_lines'] - before['source_files'][p]['code_lines'] for p in common)
    print(json.dumps({'tools': {name: version(name) for name in ['tree-sitter', 'tree-sitter-rust']},
        'python': sys.version, 'before': before, 'after': after,
        'exact_function_body_minimum_tokens': 40,
        'existing_path_code_line_delta': delta,
        'method': __doc__}, indent=2))
