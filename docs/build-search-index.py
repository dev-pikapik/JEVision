#!/usr/bin/env python3
"""Builds search-index.js, the text the search box of the documentation looks through.

The index is a plain script the pages load with <script src>, so search works without the
internet and from a page opened on disk, where a browser won't let a page read other files.
For every page in the navigation it keeps the title, the text before the first section and each
section: an h2 or h3 with an id, with the text up to the next one.

Run it after editing a page: python3 docs/build-search-index.py
check-links.py fails while the index doesn't match the pages.
"""

from html.parser import HTMLParser
from pathlib import Path
import json

SITE = Path(__file__).resolve().parent
INDEX = SITE / "search-index.js"
SKIP = {"nav", "script", "style", "template"}  # the "On this page" list repeats the headings
VOID = {"area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track", "wbr"}


class Page(HTMLParser):
    def __init__(self):
        super().__init__()
        self.nav = []
        self.title = None
        self.sections = [{"id": "", "title": [], "text": []}]
        self._in_nav = False
        self._stack = None  # open elements inside <main>; None outside it
        self._heading = None  # the h1, h2 or h3 whose text is being read
        self._skip = 0

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag == "nav" and "site-nav" in (attrs.get("class") or "").split():
            self._in_nav = True
        elif tag == "a" and self._in_nav:
            self.nav.append(attrs["href"])
        elif tag == "main":
            self._stack = []
        elif self._stack is not None and tag not in VOID:
            if not self._stack and tag in ("h2", "h3") and attrs.get("id"):
                self.sections.append({"id": attrs["id"], "title": [], "text": []})
                self._heading = tag
            elif tag == "h1" and self.title is None:
                self.title = []
                self._heading = tag
            if tag in SKIP:
                self._skip += 1
            self._stack.append(tag)

    def handle_endtag(self, tag):
        if tag == "nav" and self._in_nav:
            self._in_nav = False
        elif tag == "main":
            if self._stack:
                raise ValueError(f"<{self._stack[-1]}> isn't closed before </main>")
            self._stack = None
        elif self._stack is not None and tag not in VOID:
            if not self._stack or self._stack[-1] != tag:
                raise ValueError(f"</{tag}> at line {self.getpos()[0]} doesn't close the last open element")
            self._stack.pop()
            if tag in SKIP:
                self._skip -= 1
            if tag == self._heading:
                self._heading = None

    def handle_data(self, data):
        # Outside <main>, or between its elements, there is nothing to find.
        if not self._stack or self._skip:
            return
        if self._heading == "h1":
            self.title.append(data)
        elif self._heading:
            self.sections[-1]["title"].append(data)
            return
        self.sections[-1]["text"].append(data)


def words(parts):
    return " ".join(" ".join(parts).split())


def parse(path):
    page = Page()
    try:
        page.feed(path.read_text(encoding="utf-8"))
        page.close()
    except ValueError as error:
        raise SystemExit(f"{path.name}: {error}")
    return page


def render():
    """The text of search-index.js for the pages as they are now."""
    pages = []
    for url in parse(SITE / "index.html").nav:
        page = parse(SITE / url)
        title = words(page.title or [url])
        sections = [{"id": s["id"], "title": words(s["title"]) or title, "text": words(s["text"])} for s in page.sections]
        pages.append({"url": url, "title": title, "sections": sections})
    data = json.dumps(pages, ensure_ascii=False, indent=1)
    return (
        "// The text the search box looks through. Made by build-search-index.py; don't edit it by hand.\n"
        f"window.searchIndex = {data};\n"
    )


if __name__ == "__main__":
    INDEX.write_text(render(), encoding="utf-8", newline="\n")
    print(f"Wrote {INDEX.name}")
