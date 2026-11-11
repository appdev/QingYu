import type Vditor from 'vditor';
import alertIcons from './alert-icons.json';
import { domDefinitions, markdownDefinitions, restoreDefinitions, withoutDefinitionCarets } from './rich-references';
import { hideInlineHTML, showInlineHTML } from './rich-inline-html';

export function renderRichHTML(html: string) {
  if (!html.includes('html-inline') && !html.includes('footnotes-block')) return html;
  const template = document.createElement('template');
  template.innerHTML = html;
  let changed = false;
  for (const code of template.content.querySelectorAll('code[data-type="html-inline"]')) {
    if (code.closest('pre, [data-type="html-block"]')) continue;
    // Only a bare line-break tag is promoted; all other HTML stays in the SDK model.
    const source = (code.textContent ?? '').replace(/\u200b/g, '').trim();
    if (!/^<br\s*\/?>$/i.test(source)) continue;
    const br = document.createElement('br');
    // Outside tables Lute serializes an ordinary br as a soft break; restore the original HTML before conversion.
    if (!code.closest('th,td')) br.dataset.richHtmlBreak = source;
    code.replaceWith(br);
    changed = true;
  }
  for (const block of [...template.content.children].filter(node => node.getAttribute('data-type') === 'footnotes-block')) {
    if (block === template.content.lastElementChild) continue;
    template.content.append(block);
    changed = true;
  }
  return changed ? template.innerHTML : html;
}

function restoreHTMLBreaks(html: string) {
  if (!html.includes('data-rich-html-break')) return html;
  const template = document.createElement('template');
  template.innerHTML = html;
  for (const br of template.content.querySelectorAll<HTMLBRElement>('br[data-rich-html-break]')) {
    if (br.closest('pre, [data-type="html-block"]')) continue;
    const source = br.dataset.richHtmlBreak ?? '';
    if (!/^<br\s*\/?>$/i.test(source)) continue;
    const code = document.createElement('code');
    code.dataset.type = 'html-inline';
    code.textContent = '\u200b' + source;
    br.replaceWith(code);
  }
  return template.innerHTML;
}

export function installRichMarkdown(lute: Vditor['vditor']['lute']) {
  // Apply after each public conversion, including typing/paste normalization, not just initial import.
  for (const method of ['Md2VditorDOM', 'HTML2VditorDOM', 'SpinVditorDOM'] as const) {
    const convert = lute[method].bind(lute);
    lute[method] = (source: string) => {
      const definitions = method === 'Md2VditorDOM' ? markdownDefinitions(source) : domDefinitions(source);
      const input = method === 'SpinVditorDOM' ? withoutDefinitionCarets(restoreHTMLBreaks(hideInlineHTML(source))) : source;
      return showInlineHTML(renderRichHTML(restoreDefinitions(convert(input), definitions)));
    };
  }
  const serialize = lute.VditorDOM2Md.bind(lute);
  lute.VditorDOM2Md = (html: string) => serialize(restoreHTMLBreaks(hideInlineHTML(html)));
  const renderHTML = lute.VditorDOM2HTML.bind(lute);
  lute.VditorDOM2HTML = html => renderHTML(hideInlineHTML(html));
}

export function referenceHref(root: HTMLElement, reference: HTMLElement, lute: Vditor['vditor']['lute']) {
  const definitions = [...root.children].filter(node => node.getAttribute('data-type') === 'link-ref-defs-block').map(node => node.outerHTML).join('');
  const template = document.createElement('template');
  template.innerHTML = lute.VditorDOM2HTML(`<p data-block="0">${reference.outerHTML}</p>${definitions}`);
  return template.content.querySelector('a')?.getAttribute('href') || undefined;
}

export function decorateCallouts(root: HTMLElement) {
  for (const info of root.querySelectorAll<HTMLElement>('.callout > .callout-info')) {
    const kind = info.parentElement?.dataset.subtype as keyof typeof alertIcons;
    if (!Object.hasOwn(alertIcons, kind)) continue;
    const marker = info.querySelector('.vditor-wysiwyg__callout-marker')?.textContent ?? '';
    info.dataset.richTitle = (info.textContent ?? '').replace(marker, '').replace(/^(?:✏️|💡|❗|⚠️|🚨)\s*/u, '');
    const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16"><path d="${alertIcons[kind]}"/></svg>`;
    info.style.setProperty('--callout-icon', `url("data:image/svg+xml,${encodeURIComponent(svg)}")`);
  }
}
