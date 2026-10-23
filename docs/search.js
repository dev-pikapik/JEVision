// Search across the pages listed in the navigation. Their text comes from search-index.js, a
// script loaded before this one, so search works offline and on pages opened from disk.
(() => {
  const form = document.querySelector(".search");
  if (!form || !window.searchIndex) return;

  const input = form.querySelector("input");
  const list = form.querySelector(".search-results");
  const status = form.querySelector(".search-status-live");
  const index = window.searchIndex.flatMap((page) => page.sections.map((s) => ({
    href: s.id ? `${page.url}#${s.id}` : page.url,
    title: s.title,
    page: page.title,
    titleLower: s.title.toLowerCase(),
    textLower: s.text.toLowerCase(),
  })));

  form.hidden = false;
  form.addEventListener("submit", (event) => event.preventDefault());

  function search(query) {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    if (!words.length) return [];
    return index
      .map((s) => {
        let score = 0;
        for (const w of words) {
          if (s.titleLower.includes(w)) score += 10;
          else if (s.textLower.includes(w)) score += 1;
          else return null;
        }
        return { s, score };
      })
      .filter(Boolean)
      .sort((a, b) => b.score - a.score)
      .slice(0, 12)
      .map((r) => r.s);
  }

  function show(results, query) {
    list.replaceChildren(...results.map((s) => {
      const li = document.createElement("li");
      const a = document.createElement("a");
      a.href = s.href;
      a.textContent = s.title;
      if (s.title !== s.page) {
        const where = document.createElement("small");
        where.textContent = s.page;
        a.append(where);
      }
      li.append(a);
      return li;
    }));
    if (!query) {
      status.textContent = "";
    } else if (!results.length) {
      status.textContent = `Nothing found for “${query}”.`;
      const li = document.createElement("li");
      li.className = "search-status";
      li.textContent = status.textContent;
      list.append(li);
    } else {
      status.textContent = `${results.length} result${results.length === 1 ? "" : "s"}.`;
    }
  }

  function update() {
    const query = input.value.trim();
    show(search(query), query);
  }
  input.addEventListener("input", update);
  input.addEventListener("focus", () => input.value && update());

  function close() {
    input.value = "";
    show([], "");
  }

  form.addEventListener("keydown", (event) => {
    const links = [...list.querySelectorAll("a")];
    const at = links.indexOf(document.activeElement);
    if (event.key === "Escape") {
      close();
      input.focus();
    } else if (event.key === "ArrowDown" && links.length) {
      event.preventDefault();
      links[Math.min(at + 1, links.length - 1)].focus();
    } else if (event.key === "ArrowUp" && at >= 0) {
      event.preventDefault();
      (at === 0 ? input : links[at - 1]).focus();
    }
  });

  form.addEventListener("focusout", (event) => {
    if (!form.contains(event.relatedTarget)) list.replaceChildren();
  });

  document.addEventListener("keydown", (event) => {
    const typing = /^(INPUT|TEXTAREA|SELECT)$/.test(document.activeElement?.tagName ?? "");
    if (event.key === "/" && !typing && !event.metaKey && !event.ctrlKey) {
      event.preventDefault();
      input.focus();
    }
  });
})();
