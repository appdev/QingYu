/* Resolve original MiaoYan heading text and common lowercase/hyphen aliases without changing editor IDs. */
(() => {
  function find(root, href) {
    if (!href?.startsWith('#') || href.length === 1) return null;
    let id = href.slice(1);
    try { id = decodeURIComponent(id); } catch { return null; }
    const headings = [...root.querySelectorAll('h1,h2,h3,h4,h5,h6')];
    const ids = new Set(), slugs = new Set(), originals = new Map(), aliases = new Map();
    for (const heading of headings) {
      const text = heading.textContent.replace(/\u200b/g, '').trim();
      let name = text, count = 1;
      while (ids.has(name)) name = `${text}-${count++}`;
      ids.add(name);
      originals.set(name, heading);
      const base = text.toLowerCase().replace(/[^\p{L}\p{N}\p{M}_\-\s]/gu, '').replace(/\s/g, '-');
      let slug = base;
      count = 1;
      while (slugs.has(slug)) slug = `${base}-${count++}`;
      slugs.add(slug);
      aliases.set(slug, heading);
    }
    return originals.get(id) ?? aliases.get(id) ?? headings.find(heading => heading.id === id) ?? null;
  }
  function follow(root, href) {
    const target = find(root, href);
    if (!target) return false;
    const reduce = matchMedia('(prefers-reduced-motion: reduce)').matches;
    target.scrollIntoView({ block: 'start', behavior: reduce ? 'auto' : 'smooth' });
    return true;
  }
  window.MiaoYanHeadingLinks = { find, follow };
})();
