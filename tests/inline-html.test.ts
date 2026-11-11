// @vitest-environment jsdom
import { createRequire } from 'node:module';
import { readFileSync } from 'node:fs';
import { describe, it, expect } from 'vitest';
import { installRichMarkdown } from '../src/rich-markdown';

createRequire(import.meta.url)('../node_modules/vditor/dist/js/lute/lute.min.js');
function engine() {
  const lute = Lute.New();
  lute.SetVditorWYSIWYG(true);
  lute.SetSanitize(true);
  installRichMarkdown(lute);
  return lute;
}

describe('inline HTML production display/dehydrate integration', () => {
  for (const source of [
    '<mark>高亮</mark> H<sub>2</sub>O x<sup>2</sup> <u>下划线</u> <kbd>Ctrl</kbd>\n',
    '<MARK>**粗体** <u>嵌套 <kbd>键盘</kbd></u> 尾部</MARK>\n',
    '- <mark>列表 <sub>脚标</sub></mark>\n',
    '> <u>引用<br>换行</u>\n',
    '| A |\n|---|\n| <mark>表格<br>换行</mark> |\n',
    '<mark></mark> ==字面== ~字面~ ^字面^\n',
  ]) it(`preserves native tags after repeated edits and conversion: ${source}`, () => {
    const lute = engine();
    let dom = lute.Md2VditorDOM(source);
    const root = document.createElement('div');
    root.innerHTML = dom;
    expect(root.querySelector('[data-rich-html-open]')).not.toBeNull();
    root.querySelector('[data-rich-html-open]')!.append('追加');
    dom = root.innerHTML;
    for (let n = 0; n < 3; n++) dom = lute.SpinVditorDOM(dom);
    const markdown = lute.VditorDOM2Md(dom);
    expect(markdown).not.toMatch(/data-rich-|\u200b/);
    expect(markdown).toContain('追加');
    expect(markdown.match(/<\/?(?:mark|sub|sup|u|kbd)\s*>/gi)).toEqual(source.match(/<\/?(?:mark|sub|sup|u|kbd)\s*>/gi));
    expect(lute.VditorDOM2Md(lute.Md2VditorDOM(markdown))).toBe(markdown);
    expect(lute.VditorDOM2HTML(dom)).not.toContain('data-rich-html');
  });

  it('leaves code, attributes, unsafe, unpaired and crossing HTML in the original model', () => {
    const plain = Lute.New();
    plain.SetVditorWYSIWYG(true); plain.SetSanitize(true);
    const lute = engine();
    for (const source of [
      '`<mark>x</mark>`\n', '```html\n<mark>x</mark>\n```\n',
      '<mark class="x">x</mark>\n', '<mark onclick="alert(1)">x</mark>\n',
      '<script>alert(1)</script>\n', '<mark>x\n', '<mark>x<u>y</mark>z</u>\n',
      '<span style="color:red">red</span>\n',
    ]) {
      expect(lute.VditorDOM2Md(lute.Md2VditorDOM(source))).toBe(plain.VditorDOM2Md(plain.Md2VditorDOM(source)));
      expect(lute.Md2VditorDOM(source)).not.toContain('data-rich-html-open');
    }
  });

  it('promotes native HTML paste after SDK sanitization without changing literal extension syntax', () => {
    const lute = engine();
    const dom = lute.HTML2VditorDOM('<p><mark>高亮</mark> <u>下划线</u> <kbd>Ctrl</kbd> H<sub>2</sub>O x<sup>2</sup></p>');
    expect(dom.match(/data-rich-html-open/g)).toHaveLength(5);
    expect(lute.VditorDOM2Md(lute.SpinVditorDOM(dom))).toBe('<mark>高亮</mark> <u>下划线</u> <kbd>Ctrl</kbd> H<sub>2</sub>O x<sup>2</sup>\n');
    const source = '<mark>A &amp; B &#60; C</mark> ==字面== ~字面~ ^字面^\n';
    expect(lute.VditorDOM2Md(lute.SpinVditorDOM(lute.Md2VditorDOM(source)))).toBe(source);
  });

  it('keeps Markdown stable after the first SDK normalization of the same 37-format comparison document', () => {
    const source = readFileSync('artifacts/format-comparison/全格式对照.md', 'utf8');
    const lute = engine();
    // SVG data images have an existing sanitizer/title normalization limit; this does not assert full-source fidelity.
    let dom = lute.SpinVditorDOM(lute.Md2VditorDOM(source));
    const initial = lute.VditorDOM2Md(dom);
    for (let n = 0; n < 3; n++) dom = lute.SpinVditorDOM(dom);
    expect(lute.VditorDOM2Md(dom)).toBe(initial);
    expect(dom).toContain('data-rich-html-open');
  });
});
