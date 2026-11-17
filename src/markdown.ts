// HTML rendering for note pictures and template examples. The editor uses Markra.
import MarkdownIt from 'markdown-it';
import footnote from 'markdown-it-footnote';
import taskLists from 'markdown-it-task-lists';
import { parseMarkdownCalloutMarker } from '@markra/shared';
import alertIcons from './alert-icons.json';
import { emphasisLanguage } from './emphasis';
import { markdownBody } from './document-text';
import { markdownImageSizeSuffix } from './vendor/markra/editor/codemirror/syntax';
export { markdownBody } from './document-text';

const md = new MarkdownIt({ html: true, breaks: false, linkify: false, typographer: false })
  .use(footnote)
  .use(taskLists);

// Match MiaoYan: repeated references share a plain numeric caption, with no separator before definitions.
md.renderer.rules.footnote_caption = (tokens, index) => String(tokens[index].meta.id + 1);
md.renderer.rules.footnote_block_open = () => '<section class="footnotes">\n<ol class="footnotes-list">\n';
const footnoteAnchor = md.renderer.rules.footnote_anchor!;
md.renderer.rules.footnote_anchor = (tokens, index, options, env, renderer) => {
  const repeat = tokens[index].meta.subId;
  return footnoteAnchor(tokens, index, options, env, renderer).replace('↩︎', repeat > 0 ? `↩<sup class="footnote-ref">${repeat + 1}</sup>` : '↩');
};

// Keep math intact until the original MiaoYan KaTeX pass runs in the preview.
md.inline.ruler.before('escape', 'math-source', (state, silent) => {
  const start = state.pos;
  const source = state.src;
  const left = source.startsWith('\\(', start) ? '\\(' : source[start] === '$' ? '$' : '';
  if (!left || (left === '$' && /[\s$\d]/.test(source[start + 1] ?? ''))) return false;
  const right = left === '$' ? '$' : '\\)';
  let end = source.indexOf(right, start + left.length);
  while (end > 0 && source[end - 1] === '\\') end = source.indexOf(right, end + right.length);
  if (end < 0 || (left === '$' && (/\s/.test(source[end - 1]) || /\d/.test(source[end + 1] ?? '')))) return false;
  if (!silent) {
    const token = state.push('math_source', '', 0);
    token.content = source.slice(start, end + right.length);
  }
  state.pos = end + right.length;
  return true;
});
md.renderer.rules.math_source = (tokens, index) => `<span class="math-source">${md.utils.escapeHtml(tokens[index].content)}</span>`;

md.block.ruler.before('fence', 'math-block', (state, start, end, silent) => {
  const line = state.src.slice(state.bMarks[start] + state.tShift[start], state.eMarks[start]).trim();
  const left = line.startsWith('$$') ? '$$' : line.startsWith('\\[') ? '\\[' : '';
  if (!left) return false;
  const right = left === '$$' ? '$$' : '\\]';
  let last = start;
  if (!(line.length > left.length && line.endsWith(right))) {
    last++;
    while (last < end && !state.src.slice(state.bMarks[last], state.eMarks[last]).trim().endsWith(right)) last++;
    if (last === end) return false;
  }
  if (silent) return true;
  const token = state.push('math_block', 'div', 0);
  token.block = true;
  token.map = [start, last + 1];
  token.content = state.getLines(start, last + 1, 0, false).trim();
  state.line = last + 1;
  return true;
});
md.renderer.rules.math_block = (tokens, index) => {
  const token = tokens[index];
  return `<div class="math-source" data-sourcepos="${token.attrGet('data-sourcepos')}">${md.utils.escapeHtml(token.content)}</div>\n`;
};

md.core.ruler.after('inline', 'source-positions', (state) => {
  const lines = state.src.split('\n');
  for (const token of state.tokens) {
    if (!token.map || token.nesting === -1) continue;
    const [start, end] = token.map;
    token.attrSet('data-sourcepos', `${start + 1}:1-${end}:${Math.max(1, lines[end - 1]?.length ?? 1)}`);
  }
});

md.core.ruler.after('source-positions', 'github-alerts', (state) => {
  const tokens = state.tokens;
  for (let index = 0; index < tokens.length; index++) {
    const open = tokens[index];
    const inline = tokens[index + 2];
    if (open.type !== 'blockquote_open' || tokens[index + 1]?.type !== 'paragraph_open' || inline?.type !== 'inline') continue;
    const marker = parseMarkdownCalloutMarker(inline.content);
    if (!marker) continue;
    let depth = 1;
    let close = index + 1;
    for (; close < tokens.length; close++) {
      if (tokens[close].type === 'blockquote_open') depth++;
      if (tokens[close].type === 'blockquote_close' && --depth === 0) break;
      if (tokens[close].type !== 'blockquote_close') continue;
    }
    if (close === tokens.length) continue;
    const headerEnd = inline.content.indexOf('\n');
    const header = headerEnd < 0 ? inline.content : inline.content.slice(0, headerEnd);
    const caption = header.trimStart().slice(marker.source.length).trim() || marker.label;
    const icon = marker.tone === 'quote' ? '<path d="M0 3h6v7l-2 3H1l2-3H0zm10 0h6v7l-2 3h-3l2-3h-3z"/>' :
      `<path d="${alertIcons[marker.tone.toUpperCase() as keyof typeof alertIcons]}"/>`;
    open.tag = tokens[close].tag = 'div';
    open.attrJoin('class', `markdown-alert markdown-alert-${marker.tone}`);
    inline.content = headerEnd === -1 ? '' : inline.content.slice(headerEnd + 1);
    inline.children = [];
    md.inline.parse(inline.content, md, state.env, inline.children);
    const title = new state.Token('html_block', '', 0);
    title.content = `<p class="markdown-alert-title"><svg class="markdown-alert-icon" viewBox="0 0 16 16" aria-hidden="true">${icon}</svg>${md.utils.escapeHtml(caption)}</p>\n`;
    if (!inline.content) tokens.splice(index + 1, 3, title);
    else tokens.splice(index + 1, 0, title);
  }
});

md.core.ruler.after('github-alerts', 'standalone-images', ({ tokens }) => {
  const lineBoundary = (type?: string) => !type || type === 'softbreak' || type === 'hardbreak';
  for (const [index, token] of tokens.entries()) {
    if (token.type !== 'inline' || tokens[index - 1]?.type !== 'paragraph_open') continue;
    token.children?.forEach((child, offset, siblings) => {
      if (child.type === 'image' && lineBoundary(siblings[offset - 1]?.type) && lineBoundary(siblings[offset + 1]?.type)) {
        child.attrSet('data-standalone-image', '');
      }
    });
  }
});

const renderImage = md.renderer.rules.image!;
md.renderer.rules.image = (tokens, index, options, env, renderer) => {
  const token = tokens[index];
  if (token.attrGet('data-standalone-image') === null) return renderImage(tokens, index, options, env, renderer);
  const alt = renderer.renderInlineAsText(token.children ?? [], options, env);
  token.attrSet('alt', markdownImageSizeSuffix.test(token.content) ? alt.replace(markdownImageSizeSuffix, '') : alt);
  return renderer.renderToken(tokens, index, options);
};

export function renderMarkdown(source: string): { html: string; offset: number } {
  const { body, offset } = markdownBody(source);
  // Match MiaoYanCore.Emphasis.markCJK: CJK emphasis stays upright in the shared CSS.
  const html = md.render(body).replace(/<em>(.*?)<\/em>/gs, (whole, inner: string) => {
    const text = inner.replace(/<[^>]*>/g, '');
    const lang = emphasisLanguage(text);
    return lang ? `<em lang="${lang}">${inner}</em>` : whole;
  });
  return { html, offset };
}
