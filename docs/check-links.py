#!/usr/bin/env python3
"""Checks the documentation site for broken internal links.

Every href and src that points inside the site must name a file that exists and, with a
#fragment, an id on that page. Ids must be unique on a page, every page must have the
same navigation, and search-index.js must match the pages. External links (http, https, mailto)
are not checked.

Usage: python3 docs/check-links.py
Exit status: 0 when everything is fine, 1 when a problem was found.
"""

from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit
import runpy
import sys

SITE = Path(__file__).resolve().parent


class Page(HTMLParser):
    def __init__(self):
        super().__init__()
        self.ids = []
        self.links = []  # (line, url)
        self.nav = []
        self._in_nav = 0

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if attrs.get("id"):
            self.ids.append(attrs["id"])
        if tag == "nav" and "site-nav" in (attrs.get("class") or "").split():
            self._in_nav = 1
        elif tag == "nav" and self._in_nav:
            self._in_nav += 1
        for name in ("href", "src"):
            if attrs.get(name) is not None:
                self.links.append((self.getpos()[0], attrs[name]))
                if self._in_nav and name == "href":
                    self.nav.append(attrs[name])

    def handle_endtag(self, tag):
        if tag == "nav" and self._in_nav:
            self._in_nav -= 1


def parse(path):
    page = Page()
    page.feed(path.read_text(encoding="utf-8"))
    return page


def main():
    pages = {p.name: parse(p) for p in sorted(SITE.glob("*.html"))}
    problems = []
    for name, page in pages.items():
        seen = set()
        for i in page.ids:
            if i in seen:
                problems.append(f"{name}: id “{i}” is used more than once")
            seen.add(i)

        for line, url in page.links:
            parts = urlsplit(url)
            if parts.scheme or parts.netloc:
                if parts.scheme not in ("http", "https", "mailto"):
                    problems.append(f"{name}:{line}: unexpected link “{url}”")
                continue
            target = (SITE / name).parent / unquote(parts.path) if parts.path else SITE / name
            target = target.resolve()
            if SITE not in target.parents and target != SITE:
                problems.append(f"{name}:{line}: “{url}” leads outside the site")
                continue
            if not target.is_file():
                problems.append(f"{name}:{line}: “{url}” names a file that doesn't exist")
                continue
            if parts.fragment:
                ids = pages[target.name].ids if target.name in pages and target.parent == SITE else None
                if ids is None:
                    problems.append(f"{name}:{line}: “{url}” has an anchor on a file that isn't a page")
                elif unquote(parts.fragment) not in ids:
                    problems.append(f"{name}:{line}: “{url}”: there is no id “{parts.fragment}” on {target.name}")

    navs = {name: page.nav for name, page in pages.items()}
    reference = navs.get("index.html")
    for name, nav in navs.items():
        if not nav:
            problems.append(f"{name}: has no site navigation")
        elif nav != reference:
            problems.append(f"{name}: navigation differs from index.html")
    linked = set(reference or [])
    for name in pages:
        if name not in linked:
            problems.append(f"{name}: isn't in the site navigation")

    search = runpy.run_path(str(SITE / "build-search-index.py"))
    index = search["INDEX"]
    if not index.is_file() or index.read_text(encoding="utf-8") != search["render"]():
        problems.append(f"{index.name} doesn't match the pages: run python3 docs/build-search-index.py")

    for problem in problems:
        print(problem)
    links = sum(len(p.links) for p in pages.values())
    print(f"{len(pages)} pages, {links} links: " + (f"{len(problems)} problems" if problems else "all good"))
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
