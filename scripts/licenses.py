#!/usr/bin/env python3
"""Writes the licenses of the third-party code inside one of the apps, as Markdown.

Usage: scripts/licenses.py macos|ios OUTPUT

The Rust crates are the ones the app's core library is linked from: what `cargo tree --locked` finds
for the app's targets through normal dependencies. Build scripts and procedural macros run while
building and never ship, so neither they nor what only they use are listed. A crate's license is the
one it declares, with every LICENSE*, LICENCE*, COPYING*, NOTICE* and COPYRIGHT* file it ships.

Some crates declare a license but ship no text. They get the standard text of a license they offer
(license-texts/, or the MIT text of JEVision's own LICENSE under their authors' names), with a line
that says so. A crate that declares no license, or only licenses without a standard text here, stops
the script: someone has to look at it before it ships.

Each section is one license text and every component that ships it, so a crate under two licenses
appears in two sections. The output depends only on Cargo.lock, the resolved Swift packages and the
license files: the same sources always give the same file.

Format, which both apps read (Licenses.swift in each):
  ## <license>                  a section
  - <component> <version> — <source>
  ````text … ````               the text, in a fence longer than any run of backticks inside it
"""
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
STANDARD = os.path.join(ROOT, 'scripts', 'license-texts')
APPS = {
    'macos': {
        'title': 'JEVision for Mac',
        'package': 'jevision-ffi',
        'targets': ['aarch64-apple-darwin', 'x86_64-apple-darwin'],
        'swift': ['sparkle'],
    },
    'ios': {
        'title': 'JEVision Remote for iPhone',
        'package': 'jevision-mobile',
        'targets': ['aarch64-apple-ios', 'aarch64-apple-ios-sim'],
        'swift': [],
    },
}
# Swift packages the app links: their pin in Package.resolved and the license SwiftPM downloads with them.
SWIFT = {
    'sparkle': {
        'name': 'Sparkle',
        'license': 'MIT',
        'resolved': os.path.join(ROOT, 'apps', 'macos', 'Package.resolved'),
        'text': os.path.join(ROOT, 'apps', 'macos', '.build', 'artifacts', 'sparkle', 'Sparkle', 'LICENSE'),
    },
}
TEXT = re.compile(r'^(LICEN[CS]E|COPYING|NOTICE|COPYRIGHT|UNLICENSE)', re.IGNORECASE)
# Which standard text a crate without one gets, among the licenses it offers: those that need no
# copyright line first.
PREFERRED = ['Apache-2.0', 'MPL-2.0', 'MIT']


def fail(message):
    sys.exit(f'licenses.py: {message}')


def run(*args):
    result = subprocess.run(args, cwd=ROOT, capture_output=True, text=True)
    if result.returncode != 0:
        fail(f'`{" ".join(args[:2])}` failed:\n{result.stderr.strip()}')
    return result.stdout


def read(path):
    with open(path, encoding='utf-8', errors='replace') as f:
        text = f.read().replace('\r\n', '\n').replace('\r', '\n')
    return '\n'.join(line.rstrip() for line in text.strip('\n').split('\n'))


def standard_text(package, label):
    license = package['license']
    offered = re.split(r' OR |/', license) if not re.search(r' AND | WITH |\(', license) else []
    choice = next((name for name in PREFERRED if name in offered), None)
    if choice is None:
        fail(f'{label} ({license}) ships no license text, and none of its licenses has a standard text in '
             f'scripts/license-texts. Check the crate and add the text before shipping it.')
    if choice == 'MIT':
        mit = read(os.path.join(ROOT, 'LICENSE'))
        authors = ', '.join(re.sub(r'\s*<[^>]*>', '', author) for author in package.get('authors') or [])
        body = f'Copyright (c) {authors or "the " + package["name"] + " authors"}\n\n' + mit[mit.index('Permission is hereby granted'):]
    else:
        body = read(os.path.join(STANDARD, f'{choice}.txt'))
    return f'These crates ship no license text of their own. This is the standard text of {choice}, which they offer.\n\n{body}'


def crates(app):
    if not shutil.which('cargo'):
        fail('cargo is not on the PATH. Install Rust from https://rustup.rs and build again.')
    targets = [arg for target in app['targets'] for arg in ('--target', target)]
    tree = run('cargo', 'tree', '--locked', '-q', '-p', app['package'], *targets, '-e', 'normal,no-proc-macro',
               '--prefix', 'none', '--format', '{p}')
    linked = sorted({m.groups() for m in re.finditer(r'^(\S+) v(\S+)', tree, re.MULTILINE)})
    metadata = json.loads(run('cargo', 'metadata', '--locked', '--format-version', '1'))
    packages = {(p['name'], p['version']): p for p in metadata['packages']}
    # The workspace's own crates are JEVision itself.
    ours = set(metadata['workspace_members'])
    found = []
    for name, version in linked:
        label = f'{name} {version}'
        package = packages.get((name, version))
        if package is None:
            fail(f'cargo metadata has no package {label}, which cargo tree lists.')
        if package['id'] in ours:
            continue
        license = package.get('license') or (package.get('license_file') and 'See the license text')
        if not license:
            fail(f'{label} declares no license (neither `license` nor `license-file` in its Cargo.toml). '
                 'Check its terms before shipping it.')
        folder = os.path.dirname(package['manifest_path'])
        paths = [os.path.join(folder, n) for n in sorted(os.listdir(folder)) if TEXT.match(n) and os.path.isfile(os.path.join(folder, n))]
        if package.get('license_file'):
            declared = os.path.normpath(os.path.join(folder, package['license_file']))
            if declared not in paths:
                paths.append(declared)
        texts = [read(path) for path in paths] or [standard_text(package, label)]
        source = package.get('repository') or f'https://crates.io/crates/{name}'
        found += [(f'{label} — {source}', license, text) for text in texts]
    return found


def swift_packages(app):
    found = []
    for key in app['swift']:
        package = SWIFT[key]
        with open(package['resolved'], encoding='utf-8') as f:
            pins = {pin['identity']: pin for pin in json.load(f)['pins']}
        if key not in pins:
            fail(f'{package["resolved"]} has no {package["name"]}.')
        if not os.path.isfile(package['text']):
            fail(f'{package["text"]} is missing: run `swift package resolve` in apps/macos first.')
        pin = pins[key]
        found.append((f'{package["name"]} {pin["state"]["version"]} — {pin["location"]}', package['license'], read(package['text'])))
    return found


def fence(text):
    longest = max((len(ticks) for ticks in re.findall(r'`+', text)), default=0)
    return '`' * max(3, longest + 1)


def spdx(license):
    """`MIT/Apache-2.0`, `Apache-2.0 OR MIT` and `MIT OR Apache-2.0` are one choice, written one way."""
    if re.search(r' AND | WITH |\(', license):
        return license
    return ' OR '.join(sorted(set(re.split(r' OR |/', license)), key=str.lower))


def markdown(app, components):
    sections = {}
    # Texts that differ only in how their lines wrap are the same text: the first component's copy stands for them.
    for label, license, text in sorted(components, key=lambda c: c[0].lower()):
        license = spdx(license)
        key = (license, hashlib.sha256(' '.join(text.split()).encode()).hexdigest())
        sections.setdefault(key, (license, text, []))[2].append(label)
    lines = [
        '# Third-Party Licenses', '',
        f'{app["title"]} includes the open-source software below. Each section is a license text and the '
        'components that ship it.',
    ]
    for license, text, labels in sorted(sections.values(), key=lambda s: (s[0].lower(), sorted(s[2], key=str.lower))):
        mark = fence(text)
        lines += ['', f'## {license}', '']
        lines += [f'- {label}' for label in sorted(labels, key=str.lower)]
        lines += ['', mark + 'text', text, mark]
    return '\n'.join(lines) + '\n'


def main():
    if len(sys.argv) != 3 or sys.argv[1] not in APPS:
        sys.exit('usage: scripts/licenses.py macos|ios OUTPUT')
    app = APPS[sys.argv[1]]
    text = markdown(app, crates(app) + swift_packages(app))
    output = sys.argv[2]
    os.makedirs(os.path.dirname(os.path.abspath(output)), exist_ok=True)
    with open(output, 'w', encoding='utf-8') as f:
        f.write(text)


if __name__ == '__main__':
    main()
