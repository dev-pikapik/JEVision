// Search across the pages listed in the navigation. Pages are fetched on first use, so search
// works when the site is served over http(s); opened from disk, the search box stays hidden.
(() => {
  const form = document.querySelector(".search");
  if (!form || !/^https?:$/.test(location.protocol)) return;

  const input = form.querySelector("input");
  const list = form.querySelector(".search-results");
  const status = form.querySelector(".search-status-live");
  let index = null;

  form.hidden = false;
  form.addEventListener("submit", (event) => event.preventDefault());

  async function buildIndex() {
    const pages = [...document.querySelectorAll(".site-nav a")].map((a) => a.getAttribute("href"));
    const sections = [];
    await Promise.all(pages.map(async (page) => {
      try {
        const response = await fetch(page);
        if (!response.ok) return;
        const doc = new DOMParser().parseFromString(await response.text(), "text/html");
        const main = doc.querySelector("main");
        const pageTitle = main.querySelector("h1")?.textContent.trim() ?? page;
        let current = { href: page, title: pageTitle, page: pageTitle, text: "" };
        sections.push(current);
        for (const node of main.children) {
          if (/^H[23]$/.test(node.tagName) && node.id) {
            current = { href: `${page}#${node.id}`, title: node.textContent.trim(), page: pageTitle, text: "" };
            sections.push(current);
          } else {
            current.text += " " + node.textContent;
          }
        }
      } catch {
        // A page that can't be fetched is left out of the results.
      }
    }));
    for (const s of sections) {
      s.titleLower = s.title.toLowerCase();
      s.textLower = s.text.replace(/\s+/g, " ").toLowerCase();
    }
    return sections;
  }

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

  async function update() {
    index ??= await buildIndex();
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
