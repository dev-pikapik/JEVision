#!/usr/bin/env python3
"""Fails when the app's texts and Resources/Localizable.xcstrings drift apart.

Usage: scripts/check-localization.py [dir with .stringsdata]
build.sh passes the strings swiftc found while compiling. Without an argument the
sources are compiled into a temporary folder first (needs Xcode, not only the
Command Line Tools, and the core built by scripts/build-core-apple.sh).

Checks: every text in the code is in the catalog, every key in the catalog is still
used, every language has a translation with the same format arguments as the text in
the code (for keys like "flow.field.empty", its default value), plural forms
cover what the language needs, and a text with a key of its own passes the app's
language (`bundle: .app, locale: .app`) so it changes with it.

The Rust core reads the same catalog (crates/jevision-l10n). A key counts as used when
a string literal under crates/ is exactly that key; a key that the core asks for
(`tr!`, `tr_in!`, `text`, `text_in`, `Phrase::new`) or that looks like one of the
catalog's own keys (`run.target.notFound`) must be in the catalog. Texts the core
builds with `format!` from a key prefix are not supported: write every key out.

The command line (Sources/JEVisionCLI) asks the core for its texts with `words("cli.…", …)` and the
hands (Sources/JEVisionHands) with `handsWords("hands.…", …)`: the key must be a string literal, the catalog must have it in English and Russian, and a
key counts as used.
"""
import glob
import json
import os
import re
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CRATES = os.path.join(ROOT, '..', '..', 'crates')
CATALOG = os.path.join(ROOT, 'Resources', 'Localizable.xcstrings')
# The plural forms of the languages the catalog can hold; the core's `plural_category` follows the same rules.
PLURALS = {'ru': {'one', 'few', 'many', 'other'}, 'uk': {'one', 'few', 'many', 'other'},
           'be': {'one', 'few', 'many', 'other'}, 'ja': {'other'}, 'zh': {'other'}, 'ko': {'other'},
           'vi': {'other'}, 'th': {'other'}}
XCODE = '/Applications/Xcode.app/Contents/Developer'
CORE = os.path.join(ROOT, '..', '..', 'build', 'apple', 'JEVisionCore')
FFI_HEADERS = os.path.join(CORE, 'JEVisionCoreFFI.xcframework', 'macos-arm64_x86_64', 'Headers')
SPARKLE = os.path.join(ROOT, '.build', 'artifacts', 'sparkle', 'Sparkle', 'Sparkle.xcframework', 'macos-arm64_x86_64')
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
    hands = sorted(glob.glob(os.path.join(ROOT, 'Sources', 'JEVisionHands', '**', '*.swift'), recursive=True))
    subprocess.run(
        ['xcrun', 'swiftc', '-parse-as-library', '-swift-version', '6', '-module-name', 'JEVisionHands',
         '-target', TARGET, '-I', modules, '-I', FFI_HEADERS, '-emit-module', '-emit-module-path',
         os.path.join(modules, 'JEVisionHands.swiftmodule'), *hands],
        check=True, env=env)
    sources = sorted(glob.glob(os.path.join(ROOT, 'Sources', 'JEVision', '**', '*.swift'), recursive=True))
    if not os.path.isdir(SPARKLE):
        subprocess.run(['swift', 'package', 'resolve', '--package-path', ROOT], check=True, env=env)
    subprocess.run(
        ['xcrun', 'swiftc', '-wmo', '-c', '-parse-as-library', '-swift-version', '6',
         '-module-name', 'JEVision', '-target', TARGET, '-I', modules, '-I', FFI_HEADERS, '-F', SPARKLE,
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


def close_of(source, i):
    """Index of the parenthesis that closes the one before `i`, skipping texts and their \\( ) parts."""
    depth = 1
    while depth:
        c = source[i]
        if c == '"':
            i += 1
            while source[i] != '"':
                if source[i] == '\\':
                    i += 1
                    if source[i] == '(':
                        i = close_of(source, i + 1)
                i += 1
        elif c == '(':
            depth += 1
        elif c == ')':
            depth -= 1
        i += 1
    return i - 1


KEYED = re.compile(r'String\(\s*localized:\s*"[^"]*",\s*defaultValue:')


def keyed_without_app_language():
    """A text with a key of its own must pass the app's language itself (see AppLocalization.swift)."""
    problems = []
    for path in sorted(glob.glob(os.path.join(ROOT, 'Sources', 'JEVision', '**', '*.swift'), recursive=True)):
        source = open(path, encoding='utf-8').read()
        for found in KEYED.finditer(source):
            end = close_of(source, source.index('(', found.start()) + 1)
            if 'bundle: .app' not in source[found.start():end] or 'locale: .app' not in source[found.start():end]:
                line = source.count('\n', 0, found.start()) + 1
                problems.append(f'{os.path.relpath(path, ROOT)}:{line}: a text with a key needs "bundle: .app, locale: .app"')
    return problems


ESCAPES = {'n': '\n', 't': '\t', 'r': '\r', '0': '\0', '\\': '\\', '"': '"', "'": "'"}
# A test module written inside the file, which closes it; `mod tests;` pointing at tests.rs does not.
TEST_MODULE = re.compile(r'#\[cfg\(test\)\]\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{')
KEY_FIRST = re.compile(r'(?:\btr!|\bl10n::text|\bPhrase::new)\s*\(\s*\Z')
FAILURE_CODE = re.compile(r'\bFailure::new\s*\(\s*\Z')
KEY_AFTER_LANGUAGE = re.compile(r'(?:\btr_in!|\btext_in)\s*\([^,;]*,\s*\Z')
# A key written as `format!(...)`: it never appears whole in the code, so nothing can tell
# whether the catalog has it.
FORMATTED_KEY = re.compile(
    r'(?:\btr!\s*\(|\bl10n::text(?:_in)?\s*\(|\bPhrase::new\s*\(|\btr_in!\s*\([^,;]*,|\btext_in\s*\([^,;]*,)\s*&?\s*format!')


def unescape(text):
    out, i = [], 0
    while i < len(text):
        c = text[i]
        if c != '\\':
            out.append(c)
            i += 1
        elif text[i + 1] == 'u':
            end = text.index('}', i)
            out.append(chr(int(text[i + 3:end], 16)))
            i = end + 1
        elif text[i + 1] == '\n':
            i += 2
            while i < len(text) and text[i] in ' \t\r\n':
                i += 1
        else:
            out.append(ESCAPES.get(text[i + 1], text[i + 1]))
            i += 2
    return ''.join(out)


def rust_literals(source):
    """(value, offset) of every string literal in Rust source, comments and char literals skipped."""
    found, i, n = [], 0, len(source)
    while i < n:
        c = source[i]
        if source.startswith('//', i):
            i = source.find('\n', i)
            i = n if i < 0 else i
        elif source.startswith('/*', i):
            depth = 0
            while i < n:
                if source.startswith('/*', i):
                    depth, i = depth + 1, i + 2
                elif source.startswith('*/', i):
                    depth, i = depth - 1, i + 2
                    if not depth:
                        break
                else:
                    i += 1
        elif c == "'":
            # `'a'` and `'\n'` are characters, `'a` is a lifetime.
            if source[i + 1] == '\\':
                i = source.index("'", i + 2) + 1
            elif source[i + 2:i + 3] == "'":
                i += 3
            else:
                i += 1
        elif c == '"' or (c == 'r' and not (source[i - 1].isalnum() or source[i - 1] == '_')
                          and re.match(r'r#*"', source[i:i + 40])):
            start = i
            if c == 'r':
                hashes = re.match(r'r(#*)"', source[i:]).group(1)
                body = i + 2 + len(hashes)
                end = source.index('"' + hashes, body)
                found.append((source[body:end], start))
                i = end + 1 + len(hashes)
            else:
                i += 1
                while source[i] != '"':
                    i += 2 if source[i] == '\\' else 1
                found.append((unescape(source[start + 1:i]), start))
                i += 1
        else:
            i += 1
    return found


def rust_sources():
    """Product code of the core: files under crates/ without their tests."""
    for path in sorted(glob.glob(os.path.join(CRATES, '*', 'src', '**', '*.rs'), recursive=True)):
        if os.path.basename(path) == 'tests.rs':
            continue
        source = open(path, encoding='utf-8').read()
        inline = TEST_MODULE.search(source)
        yield os.path.relpath(path, os.path.join(ROOT, '..', '..')), source[:inline.start()] if inline else source


def rust_keys(catalog_keys):
    """(keys the core's code uses, problems with keys it asks for that the catalog lacks)."""
    sources = list(rust_sources())
    literals = {where: rust_literals(source) for where, source in sources}
    used = {value for found in literals.values() for value, _ in found if value in catalog_keys}
    # A failure code is a key, or a code the runner's phrase table (words.rs) picks a key for.
    phrased = {value for where, found in literals.items() if where.endswith('jevision-runner/src/words.rs')
               for value, _ in found}
    problems = []
    for where, source in sources:
        for match in FORMATTED_KEY.finditer(source):
            line = source.count('\n', 0, match.start()) + 1
            problems.append(f'{where}:{line}: a key built with format!; write every key as a literal')
        for value, offset in literals[where]:
            if value in catalog_keys or not value:
                continue
            line = source.count('\n', 0, offset) + 1
            before = source[max(0, offset - 160):offset]
            if KEY_FIRST.search(before) or KEY_AFTER_LANGUAGE.search(before):
                problems.append(f'{where}:{line}: the core asks for "{value}", which is missing from the catalog')
            elif FAILURE_CODE.search(before) and value not in phrased:
                problems.append(f'{where}:{line}: failure code "{value}" has no text in the catalog or in words.rs')
    return used, problems


def reference_specs(entry, key):
    """The format arguments the other languages must repeat: those of the English text, or of the key itself."""
    loc = entry.get('localizations', {}).get('en')
    if loc is None:
        return specs(key)
    if 'variations' in loc:
        units = [v.get('stringUnit', {}) for v in loc['variations'].get('plural', {}).values()]
        return specs(units[0].get('value', '')) if units else []
    return specs(loc.get('stringUnit', {}).get('value', ''))


def specs(text):
    # Positions like %1$ may reorder arguments; only their types and count must match.
    return sorted(re.sub(r'(?<=%)\d+\$', '', m) for m in SPEC.findall(text.replace('%%', '')))


CLI_WORDS = re.compile(r'\b(?:words|handsWords)\(\s*(?:"([^"\\]*)")?')


def cli_keys():
    """({key: where it is first used} for `words("key"…)` of the command line and `handsWords("key"…)` of the
    hands, calls without a literal key)."""
    keys, problems = {}, []
    sources = [p for part in ('JEVisionCLI', 'JEVisionHands')
               for p in glob.glob(os.path.join(ROOT, 'Sources', part, '**', '*.swift'), recursive=True)]
    for path in sorted(sources):
        source = open(path, encoding='utf-8').read()
        where = os.path.relpath(path, ROOT)
        for match in CLI_WORDS.finditer(source):
            if source[:match.start()].endswith('func '):
                continue
            line = f'{where}:{source.count(chr(10), 0, match.start()) + 1}'
            if match.group(1) is None:
                problems.append(f'{line}: words( needs its key as a string literal, so this check can see it')
            else:
                keys.setdefault(match.group(1), line)
    return keys, problems


def check(strings_dir, env):
    problems = []
    catalog = json.load(open(CATALOG, encoding='utf-8'))
    if catalog.get('sourceLanguage') != 'en':
        problems.append('catalog: sourceLanguage must be "en"')
    strings = catalog.get('strings', {})
    # Every language any text has must have all of them, so a new language is simply added to the catalog.
    languages = {'en', 'ru'} | {lang for entry in strings.values() for lang in entry.get('localizations', {})}

    with tempfile.TemporaryDirectory() as tmp:
        compiled = subprocess.run(
            ['xcrun', 'xcstringstool', 'compile', CATALOG, '--output-directory', tmp, '--dry-run'],
            capture_output=True, text=True, env=env)
        if compiled.returncode:
            problems.append(f'catalog: xcstringstool can’t compile it: {compiled.stderr.strip()}')

    problems += keyed_without_app_language()
    used = code_keys(strings_dir)
    if not used:
        problems.append(f'no texts found in {strings_dir}')
    for key, (table, where, _) in sorted(used.items()):
        if table != 'Localizable':
            problems.append(f'{where}: "{key}" is in table "{table}", expected Localizable')
        if key not in strings:
            problems.append(f'{where}: "{key}" is missing from the catalog')
    core_used, core_problems = rust_keys(set(strings))
    problems += core_problems
    cli_used, cli_problems = cli_keys()
    problems += cli_problems
    for key, where in sorted(cli_used.items()):
        if key not in strings:
            problems.append(f'{where}: "{key}" is missing from the catalog')
            continue
        for lang in ('en', 'ru'):
            if lang not in strings[key].get('localizations', {}):
                problems.append(f'{where}: "{key}" has no {lang} text in the catalog')
    for key in sorted(set(strings) - set(used) - core_used - set(cli_used)):
        problems.append(f'catalog: "{key}" is not used in the code any more')

    for key, entry in sorted(strings.items()):
        if entry.get('shouldTranslate') is False:
            continue
        localizations = entry.get('localizations', {})
        for lang in sorted(languages):
            forms = PLURALS.get(lang.split('-')[0], {'one', 'other'})
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
                elif specs(value) != (specs(used[key][2]) if key in used else reference_specs(entry, key)):
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
