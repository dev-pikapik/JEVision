#!/usr/bin/env python3
"""Checks the App Store texts of JEVision Remote in apps/ios/fastlane/metadata.

Every language folder must have the same files, each within the App Store limit; URLs must be
https, and the privacy policy they name must exist in this repository. See
apps/ios/fastlane/README.md.

Usage: python3 apps/ios/scripts/check-metadata.py
Exit status: 0 when everything is fine, 1 when a problem was found.
"""
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[3]
METADATA = ROOT / "apps/ios/fastlane/metadata"
REPOSITORY = "https://github.com/dev-pikapik/JEVision/blob/main/"

# The most App Store Connect takes, in characters; keywords are counted in bytes, so that a
# language written in more than one byte a letter fits too.
LIMITS = {
    "name.txt": 30,
    "subtitle.txt": 30,
    "promotional_text.txt": 170,
    "description.txt": 4000,
    "release_notes.txt": 4000,
}
KEYWORD_BYTES = 100
URLS = ("support_url.txt", "privacy_url.txt", "marketing_url.txt")
REQUIRED = set(LIMITS) | {"keywords.txt", "support_url.txt", "privacy_url.txt"}
REVIEW_NOTES = METADATA / "review_information/notes.txt"
REVIEW_NOTES_LIMIT = 4000


def read(path):
    return path.read_text(encoding="utf-8").strip()


def check_language(folder):
    problems = []
    files = {p.name for p in folder.iterdir() if p.is_file()}
    for name in sorted(REQUIRED - files):
        problems.append(f"{name} is missing")
    for name in sorted(files):
        text = read(folder / name)
        if not text:
            problems.append(f"{name} is empty")
            continue
        if name in LIMITS and len(text) > LIMITS[name]:
            problems.append(f"{name} has {len(text)} characters, the App Store takes {LIMITS[name]}")
        if name == "keywords.txt":
            problems += check_keywords(text)
        if name in URLS:
            problems += check_url(name, text)
    return files, problems


def check_keywords(text):
    problems = []
    size = len(text.encode("utf-8"))
    if size > KEYWORD_BYTES:
        problems.append(f"keywords.txt has {size} bytes, the App Store takes {KEYWORD_BYTES}")
    words = text.split(",")
    if any(word != word.strip() for word in words):
        problems.append("keywords.txt has spaces around a comma; they count against the limit")
    if any(not word.strip() for word in words):
        problems.append("keywords.txt has an empty keyword")
    lowered = [word.strip().lower() for word in words]
    for word in sorted({w for w in lowered if lowered.count(w) > 1}):
        problems.append(f"keywords.txt has “{word}” more than once")
    return problems


def check_url(name, text):
    if "\n" in text or " " in text or not text.startswith("https://"):
        return [f"{name} must be one https URL"]
    if text.startswith(REPOSITORY):
        path = text.removeprefix(REPOSITORY).split("#")[0]
        if not (ROOT / path).is_file():
            return [f"{name} names {path}, which is not in the repository"]
    return []


def main():
    problems = []
    languages = sorted(p for p in METADATA.iterdir() if p.is_dir() and p.name != "review_information")
    if not languages:
        problems.append(f"no language folders in {METADATA}")
    seen = {}
    for folder in languages:
        files, found = check_language(folder)
        seen[folder.name] = files
        problems += [f"{folder.name}/{problem}" for problem in found]
    every = set().union(*seen.values()) if seen else set()
    for language, files in seen.items():
        for name in sorted(every - files):
            problems.append(f"{language}/{name} is missing, other languages have it")
    if not REVIEW_NOTES.is_file() or not read(REVIEW_NOTES):
        problems.append("review_information/notes.txt is missing or empty")
    elif len(read(REVIEW_NOTES)) > REVIEW_NOTES_LIMIT:
        problems.append(f"review_information/notes.txt is longer than {REVIEW_NOTES_LIMIT} characters")

    for problem in problems:
        print(f"metadata: {problem}", file=sys.stderr)
    if problems:
        return 1
    print(f"Metadata: OK ({', '.join(seen)})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
