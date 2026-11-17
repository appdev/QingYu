// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
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
