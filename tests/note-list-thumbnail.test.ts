// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
// jsdom 没有 matchMedia，preferences 在导入时就会用到。
vi.hoisted(() => { Object.assign(globalThis, { matchMedia: () => ({ matches: false, addEventListener() {}, removeEventListener() {}, addListener() {} }) }); });
import { readFileSync } from 'node:fs';
import { firstLocalImage } from '../src/document-text';
import { Notebook } from '../src/notebook';
import { createSidebar } from '../src/sidebar';

beforeEach(() => {
  document.body.innerHTML = readFileSync('index.html', 'utf8').match(/<body[^>]*>([\s\S]*)<\/body>/)![1];
  localStorage.clear(); vi.stubGlobal('CSS', { escape: (text: string) => text });
});
afterEach(() => { document.body.replaceChildren(); vi.unstubAllGlobals(); });

it('finds the first local picture outside code blocks', () => {
  expect(firstLocalImage('# 标题\n\n文字\n\n![日落](assets/sunset.png)\n\n![第二张](b.jpg)')).toBe('assets/sunset.png');
  expect(firstLocalImage('![空格](<my photos/a b.jpeg>)')).toBe('my photos/a b.jpeg');
  expect(firstLocalImage('<img alt="x" src="assets/c.webp">')).toBe('assets/c.webp');
  expect(firstLocalImage('```md\n![代码里](a.png)\n```\n\n![正文](real.png)')).toBe('real.png');
  expect(firstLocalImage('![网络](https://example.com/a.png)\n\n[附件](report.pdf)')).toBeUndefined();
  expect(firstLocalImage('---\ncover: x.png\n---\n没有图片')).toBeUndefined();
});

it('shows the first picture at the end of a note row and drops it when it cannot load', () => {
  const book = new Notebook({ getItem: () => null, setItem: () => undefined }, '# 带图笔记\n\n![](pic.png)');
  const plain = { ...book.current!, id: 'plain', title: '纯文字', source: '# 纯文字\n\n没有图片' };
  book.data.notes.push(plain);
  const thumbnail = vi.fn((note: { id: string; source: string }) => {
    const image = firstLocalImage(note.source);
    return image ? `qingyu-asset://${note.id}?path=${image}` : undefined;
  });
  createSidebar(() => book, { async select() { return true; }, async create() { return true; }, async save() { return true; }, reload() {}, error: vi.fn(), thumbnail });
  const row = (id: string) => document.querySelector<HTMLElement>(`.note-row[data-note-id="${id}"]`)!;
  const image = row(book.current!.id).querySelector<HTMLImageElement>('.note-row-thumbnail')!;
  expect(image.getAttribute('src')).toBe(`qingyu-asset://${book.current!.id}?path=pic.png`);
  expect(image.alt).toBe('');
  expect(row(book.current!.id).hasAttribute('data-thumbnail')).toBe(true);
  expect(row('plain').querySelector('.note-row-thumbnail')).toBeNull();
  image.dispatchEvent(new Event('error'));
  expect(row(book.current!.id).querySelector('.note-row-thumbnail')).toBeNull();
  expect(row(book.current!.id).hasAttribute('data-thumbnail')).toBe(false);
});

it('marks shared notes with a link icon before the date, like Pinto, but not unshared or trashed ones', () => {
  const book = new Notebook({ getItem: () => null, setItem: () => undefined }, '# 已分享');
  const url = 'https://share.example.com/n/a';
  book.data.notes[0].share = { status: 'shared', url };
  book.data.notes.push({ ...book.current!, id: 'off', title: '未分享', share: { status: 'unshared', url } });
  const actions = { async select() { return true; }, async create() { return true; }, async save() { return true; }, reload() {}, error: vi.fn() };
  const sidebar = createSidebar(() => book, actions);
  expect(document.querySelector('.note-row-shared')).toBeNull();
  vi.stubGlobal('__TAURI_INTERNALS__', {});
  sidebar.refresh();
  const row = (id: string) => document.querySelector<HTMLElement>(`.note-row[data-note-id="${id}"]`)!;
  const shared = row(book.data.notes[0].id), date = shared.querySelector('.note-row-date')!;
  expect([...date.children].map(child => child.tagName.toLowerCase())).toEqual(['svg', 'time']);
  expect(date.querySelector('svg')!.classList).toContain('note-row-shared');
  expect(date.querySelector('svg')!.getAttribute('aria-hidden')).toBe('true');
  expect(shared.hasAttribute('data-shared')).toBe(true); expect(shared.getAttribute('aria-description')).toBe('已分享');
  expect(row('off').querySelector('.note-row-shared')).toBeNull(); expect(row('off').hasAttribute('data-shared')).toBe(false);
});

it('pins one date heading at a time and swaps it for the next one in place, as Pinto’s floating group rows do', async () => {
  const book = new Notebook({ getItem: () => null, setItem: () => undefined }, '# 今天的笔记');
  const day = 864e5;
  book.data.notes.push({ ...book.current!, id: 'old', title: '旧笔记', source: '# 旧笔记', updatedAt: Date.now() - 40 * day });
  createSidebar(() => book, { async select() { return true; }, async create() { return true; }, async save() { return true; }, reload() {}, error: vi.fn() });
  const list = document.querySelector<HTMLElement>('#notes-list')!, pane = document.querySelector<HTMLElement>('#notes-pane')!;
  const headings = [...list.querySelectorAll<HTMLElement>('.notes-year')];
  expect(headings).toHaveLength(2);
  // jsdom 没有布局：列表从 0 开始，内边距 56px；两个标题都停在吸顶位置（56 - 4 = 52）。
  list.style.paddingTop = '56px';
  const box = (top: number, height: number) => () => ({ top, bottom: top + height, left: 0, right: 300, width: 300, height, x: 0, y: top, toJSON() {} });
  list.getBoundingClientRect = box(0, 800); pane.getBoundingClientRect = box(0, 800);
  for (const heading of headings) { heading.style.top = '-4px'; heading.getBoundingClientRect = box(52, 40); }
  const settle = async () => { list.dispatchEvent(new Event('scroll')); await new Promise(resolve => requestAnimationFrame(resolve)); };

  list.scrollTop = 0; await settle();
  expect(headings.map(heading => [heading.hasAttribute('data-stuck'), heading.hasAttribute('data-covered')])).toEqual([[false, false], [false, false]]);
  expect(pane.style.getPropertyValue('--notes-edge')).toBe('');

  list.scrollTop = 300; await settle();
  // 后到顶的标题直接盖住前一个，前一个隐藏，不会被往上推。
  expect(headings.map(heading => [heading.hasAttribute('data-stuck'), heading.hasAttribute('data-covered')])).toEqual([[false, true], [true, false]]);
  // 模糊区延伸到吸顶标题下方 14px。
  expect(pane.style.getPropertyValue('--notes-edge')).toBe('106px');
});
