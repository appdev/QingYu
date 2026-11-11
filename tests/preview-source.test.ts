// @vitest-environment jsdom
import { createRequire } from 'node:module';
import { expect, it } from 'vitest';
import { installRichMarkdown } from '../src/rich-markdown';
import { previewPositions } from '../src/preview-source';

createRequire(import.meta.url)('../node_modules/vditor/dist/js/lute/lute.min.js');
function preview(source: string) {
  const lute = Lute.New();
  lute.SetVditorWYSIWYG(true); lute.SetSanitize(true); lute.SetFootnotes(true);
  installRichMarkdown(lute);
  const root = document.createElement('div'); root.innerHTML = lute.Md2VditorDOM(source);
  previewPositions(root, source, lute);
  return root;
}
it('maps headings, repeated paragraphs and code blocks to their original source lines', () => {
  const root = preview('# 标题\n\n重复\n\n重复\n\n```md\n- [ ] 不是任务\n```\n');
  expect([...root.children].map(node => (node as HTMLElement).dataset.sourceLine)).toEqual(['1', '3', '5', '7']);
  expect(root.querySelector('input')).toBeNull();
});
it('maps ordered, quoted and nested task checkboxes without confusing fenced code or reference definitions', () => {
  const root = preview('- [ ] 第一项\n  - [x] 嵌套\n\n> 1. [ ] 引用\n\n```md\n- [ ] 不是任务\n```\n\n[ref]: https://example.com "title"\n');
  expect([...root.querySelectorAll<HTMLInputElement>('input')].map(node => [node.dataset.sourceLine, node.disabled])).toEqual([['1', false], ['2', false], ['4', false]]);
});
it('retains reference titles and HTML while mapping, and disables unmatched task writeback', () => {
  const source = '[引用][ref] 和 <mark>高亮</mark>\n\n[ref]: https://example.com "title"\n';
  const root = preview(source);
  expect(root.querySelector('p')?.dataset.sourceLine).toBe('1');
  expect(root.querySelector('mark')?.textContent).toBe('高亮');
  const input = document.createElement('input'); input.type = 'checkbox';
  const li = document.createElement('li'); li.className = 'vditor-task'; li.append(input, 'SDK 特有任务'); root.append(li);
  const lute = Lute.New(); previewPositions(root, source, lute);
  expect(input.disabled).toBe(true); expect(input.dataset.sourceLine).toBeUndefined();
});

it('invalidates old or source-supplied task positions instead of writing to a guessed line', () => {
  const root = preview('- [ ] 任务\n');
  const input = root.querySelector<HTMLInputElement>('input')!;
  expect(input.dataset.sourceLine).toBe('1');
  const lute = Lute.New();
  previewPositions(root, '# 不相关的文档\n', lute);
  expect(input.dataset.sourceLine).toBeUndefined(); expect(input.disabled).toBe(true);
});
