// Only paired, attribute-free inline tags are promoted; other HTML stays in Vditor's source model.
const tag = /^<(\/?)(mark|sub|sup|u|kbd)\s*>$/i;
const ignored = 'pre, [data-type="html-block"]';

function htmlCode(source: string) {
  const code = document.createElement('code');
  code.dataset.type = 'html-inline';
  code.textContent = '\u200b' + source;
  return code;
}

export function showInlineHTML(html: string) {
  if (!html.includes('html-inline')) return html;
  const template = document.createElement('template');
  template.innerHTML = html;
  let changed = false;
  const stack: { node: Element; name: string; source: string }[] = [];
  for (const code of template.content.querySelectorAll('code[data-type="html-inline"]')) {
    if (code.closest(ignored)) continue;
    const source = (code.textContent ?? '').replace(/\u200b/g, '');
    const match = source.match(tag);
    if (!match) { stack.length = 0; continue; }
    const name = match[2].toLowerCase();
    if (!match[1]) { stack.push({ node: code, name, source }); continue; }
    const open = stack.pop();
    if (!open || open.name !== name || open.node.parentNode !== code.parentNode) { stack.length = 0; continue; }
    const wrapper = document.createElement(name);
    wrapper.dataset.richHtmlOpen = open.source;
    wrapper.dataset.richHtmlClose = source;
    let next = open.node.nextSibling;
    while (next && next !== code) {
      const node = next;
      next = next.nextSibling;
      wrapper.append(node);
    }
    open.node.replaceWith(wrapper);
    code.remove();
    changed = true;
  }
  return changed ? template.innerHTML : html;
}

export function hideInlineHTML(html: string) {
  if (!html.includes('data-rich-html-open')) return html;
  const template = document.createElement('template');
  template.innerHTML = html;
  let changed = false;
  for (const node of [...template.content.querySelectorAll<HTMLElement>('[data-rich-html-open]')].reverse()) {
    if (node.closest(ignored)) continue;
    const open = node.dataset.richHtmlOpen ?? '', close = node.dataset.richHtmlClose ?? '';
    const left = open.match(tag), right = close.match(tag);
    if (!left || left[1] || !right?.[1] || left[2].toLowerCase() !== right[2].toLowerCase() || node.tagName.toLowerCase() !== left[2].toLowerCase()) continue;
    node.replaceWith(htmlCode(open), ...node.childNodes, htmlCode(close));
    changed = true;
  }
  return changed ? template.innerHTML : html;
}
