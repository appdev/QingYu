// Project server attachment URLs only for display; Markdown keeps its original relative paths.
// Vditor's undo replaces DOM nodes, so remember display URLs for the active document.
const documents = new WeakMap<HTMLElement, { noteId: string; base: string; originals: Map<string, string> }>();

export function projectServerAssets(root: HTMLElement, noteId?: string, assetBase?: string) {
  if (!noteId) return;
  const base = assetBase ?? `/api/web/assets/${encodeURIComponent(noteId)}`;
  const previous = documents.get(root)?.originals;
  if (documents.get(root)?.noteId !== noteId || documents.get(root)?.base !== base) documents.set(root, { noteId, base, originals: new Map() });
  const originals = documents.get(root)!.originals;
  for (const element of root.querySelectorAll('img[src],a[href]')) {
    const attribute = element.tagName === 'IMG' ? 'src' : 'href';
    const current = element.getAttribute(attribute) ?? '';
    const source = originals.get(current) ?? previous?.get(current) ?? current;
    if (!source || /^(?:[a-z][a-z\d+.-]*:|#|\/\/)/i.test(source) || (source.startsWith('/') && !/^\/(i|files)\//.test(source))) continue;
    let path: string;
    try { path = decodeURIComponent(source.split(/[?#]/)[0]); } catch { continue; }
    const projected = `${base}${base.includes('?') ? '&' : '?'}${new URLSearchParams({ path })}`;
    originals.set(projected, source);
    if (current !== projected) element.setAttribute(attribute, projected);
  }
}

export function originalAssetPath(root: HTMLElement, href: string) { return documents.get(root)?.originals.get(href) ?? href; }

export function isLocalAsset(href: string) { return Boolean(href) && !/^(?:[a-z][a-z\d+.-]*:|#|\/\/)/i.test(href); }

export function originalAssetPaths<T>(root: HTMLElement, read: () => T): T {
  const originals = documents.get(root)?.originals;
  if (!originals) return read();
  const changed: { element: Element; attribute: string; projected: string }[] = [];
  for (const element of root.querySelectorAll('img[src],a[href]')) {
    const attribute = element.tagName === 'IMG' ? 'src' : 'href';
    const projected = element.getAttribute(attribute) ?? '', source = originals.get(projected);
    if (source !== undefined) {
      element.setAttribute(attribute, source);
      changed.push({ element, attribute, projected });
    }
  }
  try { return read(); }
  finally { for (const { element, attribute, projected } of changed) element.setAttribute(attribute, projected); }
}
