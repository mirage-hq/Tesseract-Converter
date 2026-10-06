#!/usr/bin/env python3
"""Audit conversion dependency sources without copying or staging files."""
import argparse
import json
import re
import sys
import tomllib
from pathlib import Path


def safe_path(path):
    """Reject symlinks in every component before canonicalizing a public path."""
    if any(component.is_symlink() for component in (path, *path.parents)):
        raise ValueError(f'symlink in public path: {path}')
    return path.resolve()


def workspace_for(root, manifest_path, manifest):
    explicit = manifest.get('package', {}).get('workspace')
    if explicit is not None:
        candidates = [manifest_path.parent / explicit]
    else:
        candidates = [manifest_path.parent, *manifest_path.parent.parents]
    for candidate in candidates:
        workspace = safe_path(candidate)
        if not workspace.is_relative_to(root):
            break
        workspace_manifest = safe_path(workspace / 'Cargo.toml')
        if workspace_manifest.is_file() and (explicit is not None or workspace == root or
                                             'workspace' in tomllib.loads(workspace_manifest.read_text())):
            return workspace
        if explicit is not None:
            break
    raise ValueError(f'escaping or missing workspace: {manifest_path}')


def audit_inputs(root, manifests):
    """Require literal source inputs to exist within the public workspace."""

    def check(source, relative):
        if Path(relative).is_absolute():
            raise ValueError(f'absolute source input: {source} -> {relative}')
        target = safe_path(source.parent / relative)
        if not target.is_relative_to(root) or not target.is_file():
            raise ValueError(f'escaping or missing source input: {source} -> {relative}')

    invocation = re.compile(r'\binclude(?:_str|_bytes)?!\s*\(')
    literal = re.compile(r'\s*("(?:\\.|[^"\\])*")\s*,?\s*\)')
    module_path = re.compile(r'#\s*\[\s*path\s*=\s*("(?:\\.|[^"\\])*")\s*\]')
    for manifest_path in sorted(manifests):
        manifest_path = safe_path(manifest_path)
        if not manifest_path.is_relative_to(root) or not manifest_path.is_file():
            raise ValueError(f'escaping or missing manifest: {manifest_path}')
        manifest = tomllib.loads(manifest_path.read_text())
        directory = manifest_path.parent
        package = manifest.get('package', {})
        workspace = workspace_for(root, manifest_path, manifest)
        inherited = None
        for key in ('readme', 'license-file', 'build'):
            value = package.get(key)
            if isinstance(value, str):
                check(manifest_path, value)
            elif key != 'build' and isinstance(value, dict) and value.get('workspace') is True:
                if inherited is None:
                    inherited = tomllib.loads((workspace / 'Cargo.toml').read_text()).get('workspace', {}).get('package', {})
                inherited_value = inherited.get(key)
                if not isinstance(inherited_value, str):
                    raise ValueError(f'missing inherited source input: {manifest_path}: {key}')
                check(workspace / 'Cargo.toml', inherited_value)
        if (directory / 'build.rs').exists() and package.get('build') is not False:
            check(manifest_path, 'build.rs')
        targets = [manifest.get('lib', {})]
        for kind in ('bin', 'example', 'test', 'bench'):
            targets.extend(manifest.get(kind, []))
        for target in targets:
            if 'path' in target:
                check(manifest_path, target['path'])
        for source in directory.rglob('*.rs'):
            if any(part in {'target', '.git'} for part in source.relative_to(directory).parts):
                continue
            if not safe_path(source).is_relative_to(root):
                raise ValueError(f'escaping Rust source: {source}')
            text = source.read_text()
            encoded_inputs = []
            for match in invocation.finditer(text):
                argument = literal.match(text, match.end())
                if argument is None:
                    raise ValueError(f'computed Rust include requires review: {source}')
                encoded_inputs.append(argument.group(1))
            encoded_inputs.extend(match.group(1) for match in module_path.finditer(text))
            for encoded in encoded_inputs:
                check(source, json.loads(encoded))


def audit(root, metadata):
    root = root.resolve()
    packages = metadata['packages']
    if metadata.get('resolve') is None:
        raise ValueError('resolved Cargo metadata required')
    manifests = {root / 'Cargo.toml'}
    for package in packages:
        source = package.get('source')
        if source is None:
            manifest = safe_path(Path(package['manifest_path']))
            if not manifest.is_relative_to(root) or not manifest.is_file():
                raise ValueError(f'resolved dependency outside public workspace: {manifest}')
            manifests.add(manifest)
        elif source != 'registry+https://github.com/rust-lang/crates.io-index':
            raise ValueError(f'non-crates.io resolved source: {package["name"]}: {source}')
    # Cargo's resolved graph omits optional and inactive target dependencies.
    # Inspect every local manifest, including patch crates and workspace declarations.
    manifests.update(root.glob('patches/*/Cargo.toml'))
    pending = list(manifests)
    manifests = set()
    while pending:
        manifest_path = safe_path(pending.pop())
        if not manifest_path.is_relative_to(root) or not manifest_path.is_file():
            raise ValueError(f'escaping or missing manifest: {manifest_path}')
        if manifest_path in manifests:
            continue
        manifests.add(manifest_path)
        manifest = tomllib.loads(manifest_path.read_text())
        workspace = workspace_for(root, manifest_path, manifest)
        if workspace / 'Cargo.toml' not in manifests:
            pending.append(workspace / 'Cargo.toml')
        workspace_dependencies = tomllib.loads((workspace / 'Cargo.toml').read_text()).get('workspace', {}).get('dependencies', {})
        sections = [manifest, *manifest.get('target', {}).values(), manifest.get('workspace', {})]
        for patches in manifest.get('patch', {}).values():
            sections.append({'dependencies': patches})
        for section in sections:
            for kind in ('dependencies', 'build-dependencies', 'dev-dependencies'):
                for name, value in section.get(kind, {}).items():
                    spec = value if isinstance(value, dict) else {}
                    if 'git' in spec or 'registry' in spec:
                        raise ValueError(f'non-crates.io manifest dependency: {manifest_path}: {name}')
                    if 'path' in spec:
                        target = safe_path(manifest_path.parent / spec['path'])
                        target_manifest = safe_path(target / 'Cargo.toml')
                        if not target.is_relative_to(root) or not target_manifest.is_relative_to(root) or not target_manifest.is_file():
                            raise ValueError(f'escaping or missing path dependency: {manifest_path}: {name} -> {target}')
                        pending.append(target_manifest)
                    if spec.get('workspace') and name not in workspace_dependencies:
                        raise ValueError(f'missing workspace dependency: {manifest_path}: {name}')
    audit_inputs(root, manifests)
    print(f'audited {len(manifests)} manifests and {len(packages)} resolved packages and source inputs')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--metadata', type=Path, required=True)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    try:
        audit(args.root, json.loads(args.metadata.read_text()))
    except (ValueError, KeyError, OSError, TypeError) as error:
        print(f'conversion dependency boundary: {error}', file=sys.stderr)
        sys.exit(1)
