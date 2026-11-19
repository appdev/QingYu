// @vitest-environment jsdom
import { expect, it, vi } from 'vitest';
import { pictureFilename, picturePassage } from '../src/note-picture';

vi.hoisted(() => { vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {} })); });

const table = '| 项 | 数 |\n| --- | --- |\n| 中文 | 2 |';
const fence = '```js\nconst 汉字 = 1\n```';
const note = `---\ntags: [private]\n---\n# 标题\n\n前一行内容。\n\n${table}\n\n${fence}\n\n后一行内容。\n`;

it('uses the whole note for an empty or whitespace selection without its file metadata', () => {
  const result = picturePassage(note, { from: 4, to: 19 });
  expect(result).toEqual({ text: note.slice(note.indexOf('# 标题')).trim(), startsAtTitle: true, isSelection: false });
});

it('expands cropped lines, tables and fences without exporting other text', () => {
  for (const [word, expected] of [['内容', '前一行内容。'], ['中文', table], ['汉字', fence]]) {
    const from = note.indexOf(word);
    expect(picturePassage(note, { from, to: from + word.length })).toEqual({ text: expected, startsAtTitle: false, isSelection: true });
  }
});

it('makes a bounded PNG filename without splitting a Unicode character', () => {
  expect(pictureFilename('计划/周报')).toBe('计划-周报.png');
  expect(pictureFilename(' ')).toBe('轻羽笔记.png');
  expect(pictureFilename('a'.repeat(71) + '😀尾')).toBe('a'.repeat(71) + '😀.png');
});

it('keeps dashed and bulleted list markers apart, leaving tasks and numbers alone', async () => {
  const { renderMarkdown } = await import('../src/markdown');
  const { html } = renderMarkdown('- 破折号\n  * 嵌套圆点\n\n* 圆点\n+ 加号\n\n- [ ] 任务\n- 普通项\n\n1. 编号');
  expect(html).toMatch(/<li [^>]*class="li-dash"[^>]*>破折号/);
  expect(html).toMatch(/<li [^>]*class="li-bullet"[^>]*>嵌套圆点/);
  expect(html).toMatch(/<li [^>]*class="li-bullet"[^>]*>圆点/);
  expect(html).toMatch(/<li [^>]*class="li-bullet"[^>]*>加号/);
  expect(html).toMatch(/<li [^>]*class="task-list-item"[^>]*>/);
  expect(html).not.toMatch(/task-list-item[^"]*li-/);
  expect(html).toMatch(/<li [^>]*class="li-dash"[^>]*>普通项/);
  expect(html).toMatch(/<ol[^>]*>\s*<li data-sourcepos="[^"]*">编号/);
});
