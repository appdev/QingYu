import { describe, expect, it } from 'vitest';
import { EditorSelection, EditorState } from '@codemirror/state';
import { formatTransaction, replaceSource, toggleTask } from '../src/format';
import { markdownBody, renderMarkdown } from '../src/markdown';
import { emphasisLanguage } from '../src/emphasis';

describe('legacy markdown-it comparison baseline (not the runtime preview)', () => {
  it('shares upright CJK and italic Latin emphasis rules between both modes', () => {
    for (const [text, language] of [['English emphasis', ''], ['中文 and English', 'zh'], ['こんにちは', 'ja'], ['한국어', 'ko'], ['𠀀', 'zh']]) {
      expect(emphasisLanguage(text)).toBe(language);
      expect(renderMarkdown(`*${text}*`).html).toContain(language ? `<em lang="${language}">` : '<em>');
    }
  });
  it('preserves authored Markdown and maps blocks to original source lines', () => {
    const source = '# 标题\n\n正文 **粗体**\n\n- [ ] 待办';
    const result = renderMarkdown(source);
    expect(result.html).toContain('<h1 data-sourcepos="1:1-1:4">标题</h1>');
    expect(result.html).toContain('<strong>粗体</strong>');
    expect(result.html).toContain('data-sourcepos="5:1-5:8"');
    expect(result.html).toContain('type="checkbox"');
    expect(source).toBe('# 标题\n\n正文 **粗体**\n\n- [ ] 待办');
  });
  it('strips frontmatter only when closed and accounts for its source lines', () => {
    expect(markdownBody('---\r\ntitle: 标题\r\n---\r\n# 正文')).toEqual({ body: '# 正文', offset: 3 });
    expect(markdownBody('---\ntitle: 未关闭')).toEqual({ body: '---\ntitle: 未关闭', offset: 0 });
  });
  it('keeps math source intact instead of parsing underscores as emphasis', () => {
    const { html } = renderMarkdown('行内 $x_i + x_j$ 与 \\(a_b\\)。\n\n$$\nx_i = \\frac{a_b}{c}\n$$');
    expect(html).toContain('$x_i + x_j$');
    expect(html).toContain('\\(a_b\\)');
    expect(html).toContain('x_i = \\frac{a_b}{c}');
    expect(html).not.toContain('<em>');
  });
  it('does not interpret currency as a formula', () => {
    expect(renderMarkdown('价格 $5 和 $10').html).not.toContain('math-source');
  });
  it('keeps CJK emphasis upright like MiaoYan, including mixed and nested text', () => {
    const { html } = renderMarkdown('*中文 Markdown*、*かな*、*한글*、*English*、***婴宁***、*𠀀*');
    expect(html).toContain('<em lang="zh">中文 Markdown</em>');
    expect(html).toContain('<em lang="ja">かな</em>');
    expect(html).toContain('<em lang="ko">한글</em>');
    expect(html).toContain('<em>English</em>');
    expect(html).toContain('<em lang="zh"><strong>婴宁</strong></em>');
    expect(html).toContain('<em lang="zh">𠀀</em>');
    expect(renderMarkdown('```html\n<em>中文</em>\n```').html).not.toContain('<em lang=');
  });
  it('matches MiaoYan alert containers and icons while balancing nested quotes', () => {
    const { html } = renderMarkdown('> [!NOTE]\n> 提示\n>\n> > [!TIP]\n> > 建议\n\n> 普通引用\n\n> [!WARNING]: 不是提示\n\n```md\n> [!CAUTION]\n```');
    expect(html).toContain('class="markdown-alert markdown-alert-note"');
    expect(html).toContain('class="markdown-alert markdown-alert-tip"');
    expect(html.match(/<div /g)).toHaveLength(2);
    expect(html.match(/<\/div>/g)).toHaveLength(2);
    expect(html.match(/class="markdown-alert-icon"/g)).toHaveLength(2);
    expect(html).not.toContain('markdown-alert-warning');
    expect(html).not.toContain('markdown-alert-caution');
    expect(renderMarkdown('> [!NOTE]').html).not.toContain('<p></p>');
  });
  it('keeps fenced code literal and renders tables and footnotes', () => {
    const { html } = renderMarkdown('```swift\nlet text = "<b>不是 HTML</b>"\n```\n\n| A | B |\n| - | - |\n| 1 | 2 |\n\n文字[^1]\n\n[^1]: 注释');
    expect(html).toContain('language-swift');
    expect(html).toContain('&lt;b&gt;不是 HTML&lt;/b&gt;');
    expect(html).toContain('<table');
    expect(html).toContain('footnote');
  });
  it('matches MiaoYan footnote numbering without adding a separator', () => {
    const { html } = renderMarkdown('第一处[^one]，再次[^one]，第二条[^two]。\n\n[^one]: 第一段\n\n    第二段\n\n[^two]: 第二条\n');
    const captions = [...html.matchAll(/<sup class="footnote-ref"><a[^>]*>(.*?)<\/a><\/sup>/g)].map(match => match[1]);
    expect(captions).toEqual(['1', '1', '2']);
    expect(html).not.toContain('footnotes-sep');
    expect(html).toContain('href="#fnref1:1"');
    expect(html).toContain('↩<sup class="footnote-ref">2</sup>');
    expect(html).toContain('第二段');
  });
});

describe('source edits, selection and task write-back', () => {
  it('maps visual edits into the shared source while retaining unchanged selections and Unicode', () => {
    const state = EditorState.create({ doc: '前文🌸\n\n末尾', selection: EditorSelection.cursor(8) });
    expect(replaceSource(state, state.doc.toString())).toBeNull();
    const next = replaceSource(state, '前文🌼和中文\n\n末尾')!.state;
    expect(next.doc.toString()).toBe('前文🌼和中文\n\n末尾');
    expect(next.selection.main.from).toBe(11);
    expect(replaceSource(next, '')!.state.doc.toString()).toBe('');
  });
  it('wraps Chinese and emoji without changing adjacent source', () => {
    const state = EditorState.create({ doc: '前文你好🌸后文', selection: EditorSelection.range(2, 6) });
    const next = formatTransaction(state, 'bold').state;
    expect(next.doc.toString()).toBe('前文**你好🌸**后文');
    expect(next.sliceDoc(next.selection.main.from, next.selection.main.to)).toBe('你好🌸');
  });
  it('leaves the caret inside an empty Markdown wrapper', () => {
    const state = EditorState.create({ doc: '', selection: EditorSelection.cursor(0) });
    const next = formatTransaction(state, 'link').state;
    expect(next.doc.toString()).toBe('[](https://)');
    expect(next.selection.main.from).toBe(1);
  });
  it('quotes all selected lines without consuming the following line', () => {
    const state = EditorState.create({ doc: '第一行\n第二行\n第三行', selection: EditorSelection.range(0, 8) });
    expect(formatTransaction(state, 'quote').state.doc.toString()).toBe('> 第一行\n> 第二行\n第三行');
  });
  it('changes only the clicked task and preserves the editor selection', () => {
    const state = EditorState.create({ doc: '- [ ] 第一项\n\n1. [x] 第二项', selection: EditorSelection.cursor(7) });
    const next = toggleTask(state, 3)!.state;
    expect(next.doc.toString()).toBe('- [ ] 第一项\n\n1. [ ] 第二项');
    expect(next.selection.main.from).toBe(7);
    expect(toggleTask(state, 2)).toBeNull();
    expect(toggleTask(state, 50)).toBeNull();
  });
});
