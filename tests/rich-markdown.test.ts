// @vitest-environment jsdom
import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { decorateCallouts, installRichMarkdown, renderRichHTML, referenceHref } from '../src/rich-markdown';

createRequire(import.meta.url)('../node_modules/vditor/dist/js/lute/lute.min.js');
function engine() {
  const lute = Lute.New();
  lute.SetVditorWYSIWYG(true);
  lute.SetSanitize(true);
  lute.SetCallout(true);
  lute.SetFootnotes(true);
  return lute;
}

describe('table break integration with the bundled Lute', () => {
  it('keeps table breaks through import, typing normalization, HTML paste and export', () => {
    const lute = engine();
    installRichMarkdown(lute);
    const source = '| A | B |\n|---|---|\n| 第一行<br>第二行 | **粗体<br/>换行** |\n';
    const dom = lute.Md2VditorDOM(source);
    expect(dom).toContain('第一行<br>第二行');
    expect(dom).toContain('<strong data-marker="**">粗体<br>换行</strong>');
    expect(lute.SpinVditorDOM(dom)).not.toContain('html-inline');
    const markdown = lute.VditorDOM2Md(lute.SpinVditorDOM(dom));
    expect(markdown).toContain('第一行<br />第二行');
    expect(lute.Md2VditorDOM(markdown)).toMatch(/第一行<br\s*\/>第二行|第一行<br>第二行/);
    expect(lute.HTML2VditorDOM('<table><tr><td>粘贴<br>换行</td></tr></table>')).toMatch(/粘贴<br\s*\/>换行|粘贴<br>换行/);
  });
  it('leaves other HTML, inline code, fenced code and attributed tags in the original model', () => {
    const lute = engine();
    const source = '正文 <span class="custom">标记</span>\n\n```html\n<br>\n```\n\n| A |\n|---|\n| `<br>` <br onload="bad()"> |\n\n引用[^a]\n\n[^a]: 脚注\n';
    const original = lute.Md2VditorDOM(source);
    installRichMarkdown(lute);
    expect(lute.Md2VditorDOM(source)).toBe(original);
    expect(renderRichHTML('<table><tr><td><pre><code data-type="html-inline">&lt;br&gt;</code></pre></td></tr></table>')).toContain('&lt;br&gt;');
  });
  it('retains literal HTML breaks in paragraphs, emphasis, lists and quotes across repeated normalization and export', () => {
    const lute = engine();
    installRichMarkdown(lute);
    for (const source of ['第一行<br>第二行\n', '**粗体<BR/>换行**\n', '- 列表<br>换行\n', '> 引用<br>换行\n']) {
      let dom = lute.Md2VditorDOM(source);
      expect(dom).toContain('data-rich-html-break');
      for (let i = 0; i < 3; i++) dom = lute.SpinVditorDOM(dom);
      const markdown = lute.VditorDOM2Md(dom);
      expect(markdown).toBe(source);
      expect(markdown).not.toContain('data-rich-');
      expect(lute.VditorDOM2HTML(dom)).toMatch(/<br\s*\/>|<br>/);
    }
  });
  it('moves initial footnotes to the end without changing their text or reference labels', () => {
    const lute = engine();
    installRichMarkdown(lute);
    const source = '脚注[^a]，再次[^a]\n\n[^a]: 第一段\n\n    第二段\n\n# 结束\n';
    const dom = lute.Md2VditorDOM(source);
    expect(dom.indexOf('footnotes-block')).toBeGreaterThan(dom.indexOf('结束'));
    const markdown = lute.VditorDOM2Md(lute.SpinVditorDOM(dom));
    expect(markdown.match(/\[\^a\]/g)).toHaveLength(3);
    expect(markdown).toContain('第一段');
    expect(markdown).toContain('第二段');
  });
});

it('resolves reference destinations with the SDK without converting or modifying the editable reference model', () => {
  const lute = engine();
  for (const href of ['https://example.org/path?q=1#x', '#中文标题', 'relative/note.md', 'mailto:test@example.com', 'javascript:alert(1)']) {
    const root = document.createElement('div');
    root.innerHTML = lute.Md2VditorDOM(`[引用][a]\n\n[a]: ${href}\n`);
    const before = root.innerHTML;
    const reference = root.querySelector<HTMLElement>('[data-type="link-ref"]')!;
    expect(referenceHref(root, reference, lute)).toBe(href.startsWith('javascript:') ? undefined : href);
    expect(root.innerHTML).toBe(before);
    expect(lute.VditorDOM2Md(root.innerHTML)).toContain('[引用][a]');
    root.querySelector<HTMLElement>('[data-type="link-ref-defs-block"]')!.style.display = 'none';
    expect(lute.VditorDOM2Md(root.innerHTML)).toContain(`[a]: ${href}`);
  }
});

describe('reference definition source fidelity', () => {
  for (const definition of [
    '[a]: https://example.com "中文标题"',
    "[a]: <https://example.com/path> 'single title'",
    '[a]: /path (parenthesized title)',
    '[a]: /path "escaped \\"quote\\" & <tag>"',
    '[a]: /path ""',
    '[a]: /path\n  "多行\n标题"',
  ]) it(`retains the current definition through normalization: ${definition}`, () => {
    const lute = engine();
    installRichMarkdown(lute);
    let dom = lute.Md2VditorDOM(`[引用][a]\n\n${definition}\n`);
    for (let n = 0; n < 3; n++) dom = lute.SpinVditorDOM(dom);
    expect(lute.VditorDOM2Md(dom)).toContain(`${definition}\n`);
    const root = document.createElement('div');
    root.innerHTML = dom;
    const block = root.querySelector('[data-type="link-ref-defs-block"]')!;
    block.textContent = '[a]: /changed "new title"\n';
    expect(lute.VditorDOM2Md(lute.SpinVditorDOM(root.innerHTML))).toContain('[a]: /changed "new title"');
    block.textContent = '[a]: /changed\n';
    expect(lute.VditorDOM2Md(lute.SpinVditorDOM(root.innerHTML))).not.toContain('title');
  });

  it('does not harvest definitions from code or revive removed definitions', () => {
    const lute = engine();
    installRichMarkdown(lute);
    const source = '[引用][a]\n\n[a]: /first "first"\n[b]: /second "second"\n\n```md\n[a]: /wrong "code"\n```\n';
    const root = document.createElement('div');
    root.innerHTML = lute.Md2VditorDOM(source);
    expect(lute.VditorDOM2Md(lute.SpinVditorDOM(root.innerHTML))).toContain('[a]: /first "first"');
    root.querySelector('[data-type="link-ref-defs-block"]')!.remove();
    expect(lute.VditorDOM2Md(lute.SpinVditorDOM(root.innerHTML))).not.toContain('/first');
  });

  it('preserves the current caret inside an edited title without restoring invalid definitions', () => {
    const lute = engine();
    installRichMarkdown(lute);
    const dom = '<p data-block="0">正文</p><div data-block="0" data-type="link-ref-defs-block">[a]: /a "ti<wbr>tle"\n[b]: /b \'second\'\n</div>';
    const result = lute.SpinVditorDOM(dom);
    expect(result).toContain('"ti<wbr>tle"');
    expect(lute.VditorDOM2Md(result)).toContain('[b]: /b \'second\'');
    const invalid = dom.replace('"ti<wbr>tle"', '"unterminated');
    expect(lute.VditorDOM2Md(lute.SpinVditorDOM(invalid))).toContain('"unterminated');
    expect(lute.VditorDOM2Md(lute.SpinVditorDOM(invalid)).match(/second/g)).toHaveLength(1);
  });

  it('retains SDK definitions not accepted by the reference helper without enabling unsafe links', () => {
    const lute = engine();
    installRichMarkdown(lute);
    const source = '[safe][a] [unsafe][b]\n\n[a]: /safe "title"\n[b]: javascript:alert(1) "unsafe title retained as text"\n';
    const dom = lute.SpinVditorDOM(lute.Md2VditorDOM(source));
    expect(lute.VditorDOM2Md(dom)).toContain('[b]: javascript:alert(1)');
    expect(lute.VditorDOM2Md(dom)).toContain('"unsafe title retained as text"');
    expect(lute.VditorDOM2Md(dom)).toContain('[a]: /safe "title"');
    const root = document.createElement('div'); root.innerHTML = dom;
    expect(referenceHref(root, root.querySelector('[data-link-label="b"]')!, lute)).toBeUndefined();
  });

  it('matches Unicode, whitespace, escaped labels and definitions nested inside quotes', () => {
    const lute = engine(); installRichMarkdown(lute);
    for (const source of [
      '[引用][a b]\n\n[A  B]: /x "space"\n',
      '[引用][中文]\n\n[中文]: /x "中文"\n',
      '[引用][a\\]]\n\n[a\\]]: /x "escaped"\n',
      '[引用][a]\n\n> [a]: /x\n>   \'quote\'\n',
    ]) {
      const dom = lute.SpinVditorDOM(lute.Md2VditorDOM(source));
      expect(lute.VditorDOM2Md(dom)).toMatch(/"space"|"中文"|"escaped"|'quote'/);
    }
  });

  it('does not turn a valid reference into prose when deleting the title at the destination boundary', () => {
    const lute = engine(); installRichMarkdown(lute);
    const dom = '<p data-block="0"><span data-type="link-ref" data-link-label="a">引用</span></p><div data-block="0" data-type="link-ref-defs-block">[a]: &lt;https://example.com&gt;<wbr>\n[b]: /other\n  "多行\n标题"\n</div>';
    const result = lute.SpinVditorDOM(dom);
    expect(result).toContain('&gt;<wbr>');
    expect(lute.VditorDOM2Md(result)).toBe('[引用][a]\n\n[a]: <https://example.com>\n[b]: /other\n  "多行\n标题"\n');
    const nextLine = '<div data-block="0" data-type="link-ref-defs-block">[a]: /a "A"\n<wbr>[b]: /b "B"\n</div>';
    const normalized = lute.SpinVditorDOM(nextLine);
    expect(normalized.match(/<wbr>/g)).toHaveLength(1);
    expect(normalized).toContain('<wbr>[b]');
  });
});

it('decorates all five callout titles without changing SDK text or serialized Markdown', () => {
  const lute = engine();
  const root = document.createElement('div');
  root.innerHTML = lute.Md2VditorDOM(['NOTE', 'TIP', 'IMPORTANT', 'WARNING', 'CAUTION'].map(kind => `> [!${kind}]\n> 内容\n`).join('\n'));
  const before = lute.VditorDOM2Md(root.innerHTML), text = root.textContent;
  decorateCallouts(root);
  decorateCallouts(root);
  expect([...root.querySelectorAll<HTMLElement>('[data-rich-title]')].map(node => node.dataset.richTitle)).toEqual(['Note', 'Tip', 'Important', 'Warning', 'Caution']);
  expect(root.querySelectorAll('svg')).toHaveLength(0);
  expect(root.textContent).toBe(text);
  expect(lute.VditorDOM2Md(root.innerHTML)).toBe(before);
});

describe('shared heading links', () => {
  beforeEach(() => {
    window.eval(readFileSync('public/heading-links.js', 'utf8'));
    window.matchMedia = vi.fn().mockReturnValue({ matches: true });
  });
  it('resolves original text, encoded text, lowercase aliases and duplicates without rewriting SDK IDs', () => {
    const root = document.createElement('div');
    root.innerHTML = '<h2 id="wysiwyg-1">C01 标题层级</h2><h2 id="wysiwyg-2">C01 标题层级</h2><h2 id="wysiwyg-3">标点（测试） <code>x</code></h2>';
    const before = root.innerHTML;
    const find = window.MiaoYanHeadingLinks.find;
    expect(find(root, '#C01%20%E6%A0%87%E9%A2%98%E5%B1%82%E7%BA%A7')).toBe(root.children[0]);
    expect(find(root, '#c01-标题层级')).toBe(root.children[0]);
    expect(find(root, '#c01-标题层级-1')).toBe(root.children[1]);
    expect(find(root, '#C01 标题层级-1')).toBe(root.children[1]);
    expect(find(root, '#标点测试-x')).toBe(root.children[2]);
    expect(find(root, '#wysiwyg-2')).toBe(root.children[1]);
    expect(find(root, '#missing')).toBeNull();
    expect(find(root, '#bad%XX')).toBeNull();
    expect(find(root, 'https://example.com/#c01-标题层级')).toBeNull();
    expect(root.innerHTML).toBe(before);
    const target = root.children[1] as HTMLElement;
    target.scrollIntoView = vi.fn();
    expect(window.MiaoYanHeadingLinks.follow(root, '#c01-标题层级-1')).toBe(true);
    expect(target.scrollIntoView).toHaveBeenCalledWith({ block: 'start', behavior: 'auto' });
  });
});
