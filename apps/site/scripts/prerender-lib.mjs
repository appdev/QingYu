const rootIdPattern = /\bid\s*=\s*(["'])root\1/giu;
const injectableRootPattern = /<div id="root"[^>]*><\/div>/gu;

export function injectPrerenderedRoot(html, markup) {
  const roots = html.match(rootIdPattern) ?? [];
  const injectableRoots = html.match(injectableRootPattern) ?? [];
  if (roots.length !== 1 || injectableRoots.length !== 1) {
    throw new Error("Expected exactly one root element.");
  }

  return html.replace(injectableRootPattern, () => `<div id="root">${markup}</div>`);
}
