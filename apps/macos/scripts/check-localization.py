#!/usr/bin/env python3
"""Fails when the app's texts and Resources/Localizable.xcstrings drift apart.

Usage: scripts/check-localization.py [dir with .stringsdata]
build.sh passes the strings swiftc found while compiling. Without an argument the
sources are compiled into a temporary folder first (needs Xcode, not only the
Command Line Tools, and the core built by scripts/build-core-apple.sh).

Checks: every text in the code is in the catalog, every key in the catalog is still
used, every language has a translation with the same format arguments as the text in
the code (for keys like "flow.field.empty", its default value), and plural forms
cover what the language needs.
"""
import glob
import json
import os
import re
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CATALOG = os.path.join(ROOT, 'Resources', 'Localizable.xcstrings')
LANGUAGES = {'en': {'one', 'other'}, 'ru': {'one', 'few', 'many', 'other'}}
XCODE = '/Applications/Xcode.app/Contents/Developer'
CORE = os.path.join(ROOT, '..', '..', 'build', 'apple', 'JEVisionCore')
FFI_HEADERS = os.path.join(CORE, 'JEVisionCoreFFI.xcframework', 'macos-arm64_x86_64', 'Headers')
TARGET = 'arm64-apple-macos26.0'
SPEC = re.compile(r'%(?:\d+\$)?[-+ 0#]*\d*(?:\.\d+)?(?:hh|h|ll|l|q|z|t|j)?[@dDiuUxXoOfFeEgGcCsSp]')


def tool_env():
    env = dict(os.environ)
    found = subprocess.run(['xcrun', '--find', 'xcstringstool'], capture_output=True, env=env).returncode == 0
    if not found and os.path.isdir(XCODE):
        env['DEVELOPER_DIR'] = XCODE
    return env


def extract(out_dir, env):
    core = os.path.join(CORE, 'Sources', 'JEVisionCore', 'JEVisionCore.swift')
    if not os.path.isfile(core):
        sys.exit('The core is not built yet: run scripts/build-core-apple.sh at the repository root.')
    modules = os.path.join(out_dir, 'modules')
    subprocess.run(
        ['xcrun', 'swiftc', '-parse-as-library', '-swift-version', '6', '-module-name', 'JEVisionCore',
         '-target', TARGET, '-I', FFI_HEADERS, '-emit-module', '-emit-module-path',
         os.path.join(modules, 'JEVisionCore.swiftmodule'), core],
        check=True, env=env)
    sources = sorted(glob.glob(os.path.join(ROOT, 'Sources', '**', '*.swift'), recursive=True))
    subprocess.run(
        ['xcrun', 'swiftc', '-wmo', '-c', '-parse-as-library', '-swift-version', '6',
         '-module-name', 'JEVision', '-target', TARGET, '-I', modules, '-I', FFI_HEADERS,
         '-Xfrontend', '-emit-localized-strings', '-Xfrontend', '-emit-localized-strings-path', '-Xfrontend', out_dir,
         *sources, '-o', os.path.join(out_dir, 'app.o')],
        check=True, env=env)


def code_keys(strings_dir):
    keys = {}
    for path in sorted(glob.glob(os.path.join(strings_dir, '**', '*.stringsdata'), recursive=True)):
        data = json.load(open(path, encoding='utf-8'))
        source = os.path.relpath(data.get('source', path), ROOT)
        for table, items in data.get('tables', {}).items():
            for item in items:
                line = item.get('location', {}).get('startingLine', '?')
                keys.setdefault(item['key'], (table, f'{source}:{line}', item.get('value', item['key'])))
    return keys


def specs(text):
    # Positions like %1$ may reorder arguments; only their types and count must match.
    return sorted(re.sub(r'(?<=%)\d+\$', '', m) for m in SPEC.findall(text.replace('%%', '')))


def check(strings_dir, env):
    problems = []
    catalog = json.load(open(CATALOG, encoding='utf-8'))
    if catalog.get('sourceLanguage') != 'en':
        problems.append('catalog: sourceLanguage must be "en"')
    strings = catalog.get('strings', {})

    with tempfile.TemporaryDirectory() as tmp:
        compiled = subprocess.run(
            ['xcrun', 'xcstringstool', 'compile', CATALOG, '--output-directory', tmp, '--dry-run'],
            capture_output=True, text=True, env=env)
        if compiled.returncode:
            problems.append(f'catalog: xcstringstool can’t compile it: {compiled.stderr.strip()}')

    used = code_keys(strings_dir)
    if not used:
        problems.append(f'no texts found in {strings_dir}')
    for key, (table, where, _) in sorted(used.items()):
        if table != 'Localizable':
            problems.append(f'{where}: "{key}" is in table "{table}", expected Localizable')
        if key not in strings:
            problems.append(f'{where}: "{key}" is missing from the catalog')
    for key in sorted(set(strings) - set(used)):
        problems.append(f'catalog: "{key}" is not used in the code any more')

    for key, entry in sorted(strings.items()):
        if entry.get('shouldTranslate') is False:
            continue
        localizations = entry.get('localizations', {})
        for lang, forms in LANGUAGES.items():
            loc = localizations.get(lang)
            if loc is None:
                if lang != 'en':
                    problems.append(f'{lang}: "{key}" has no translation')
                continue
            if 'variations' in loc:
                plural = loc['variations'].get('plural', {})
                missing = forms - set(plural)
                if missing:
                    problems.append(f'{lang}: "{key}" lacks plural forms {sorted(missing)}')
                units = [v.get('stringUnit', {}) for v in plural.values()]
            else:
                units = [loc.get('stringUnit', {})]
            for unit in units:
                value = unit.get('value', '')
                if not value.strip() or unit.get('state') != 'translated':
                    problems.append(f'{lang}: "{key}" is empty or not marked translated')
                elif specs(value) != specs(used.get(key, (None, None, key))[2]):
                    problems.append(f'{lang}: "{key}" → "{value}" has different format arguments')
    return problems


def main():
    env = tool_env()
    if len(sys.argv) > 1:
        problems = check(sys.argv[1], env)
    else:
        with tempfile.TemporaryDirectory() as tmp:
            extract(tmp, env)
            problems = check(tmp, env)
    for problem in problems:
        print(problem, file=sys.stderr)
    if problems:
        print(f'Localization: {len(problems)} problem(s)', file=sys.stderr)
        sys.exit(1)
    print('Localization: OK')


if __name__ == '__main__':
    main()
