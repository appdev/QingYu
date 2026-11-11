// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest';
import { createNoteMenu, noteInformation } from '../src/note-menu';
import { Notebook } from '../src/notebook';

afterEach(() => { document.body.replaceChildren(); vi.unstubAllGlobals(); });

it('matches MiaoYan CJK/Latin word counts, frontmatter exclusion and backlink title deduplication', () => {
  const book = new Notebook({ getItem: () => null, setItem: () => undefined }, "---\ntitle: 隐藏\n---\n# 中文 don't 2026 カナ 한글 🙂");
  const note = book.current!;
  note.title = '目标';
  book.data.notes.push({ ...note, id: 'ref-a', title: '引用者', source: '[[ 目标 ]] [[目标]]' }, { ...note, id: 'ref-b', title: '引用者', source: '[[目标]]' },
    { ...note, id: 'trash', title: '废纸篓引用', source: '[[目标]]', trashed: true }, { ...note, id: 'alias', title: '别名', source: '[[目标|别名]]' });
  const info = noteInformation(book.data, note.id);
  expect(info.words).toBe(8);
  expect(info.backlinks).toEqual(['引用者']);
  expect(info.modified).toMatch(/\d{4}\/\d{2}\/\d{2} \d{2}:\d{2}/);
});

it('edits the title inline, cancels with Escape, keeps source/history and dismisses the nonmodal info panel', async () => {
  vi.stubGlobal('CSS', { escape: (text: string) => text });
  const book = new Notebook({ getItem: () => null, setItem: () => undefined }, '# 正文标题\r\n\r\n正文不改\r\n');
  await book.save(book.current!.source);
  const note = book.current!, before = note.source;
  document.body.innerHTML = '<h1 id="note-title">正文标题</h1><div id="notes-list"></div>';
  const row = document.createElement('button'); row.className = 'note-row'; row.dataset.noteId = note.id; row.setAttribute('aria-selected', 'true'); document.querySelector('#notes-list')!.append(row);
  const change = vi.fn(async (mutate: () => Promise<void>) => { await mutate(); });
  const error = vi.fn(), save = vi.fn(async () => true), menu = createNoteMenu(() => book, { save, change, error });
  const open = async (name: string) => { await menu(note.id, 200, 100); [...document.querySelectorAll<HTMLButtonElement>('[role=menuitem]')].find(button => button.ariaLabel === name)!.click(); };
  await open('重命名');
  let input = document.querySelector<HTMLInputElement>('#rename-note')!;
  expect(input.parentElement!.id).toBe('note-title'); expect(document.querySelector('dialog')).toBeNull();
  input.value = '新名称'; input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
  await vi.waitFor(() => expect(book.current!.title).toBe('新名称'));
  await vi.waitFor(() => expect(document.querySelector('#rename-note')).toBeNull());
  expect(book.current!.source).toBe(before); expect(change.mock.calls[0]).toHaveLength(1);
  await open('重命名'); input = document.querySelector<HTMLInputElement>('#rename-note')!;
  input.value = '取消的名称'; input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  expect(book.current!.title).toBe('新名称'); expect(document.querySelector('#note-title')!.textContent).toBe('新名称');
  await open('显示简介');
  await vi.waitFor(() => expect(document.querySelector('.note-info')).not.toBeNull());
  expect(document.querySelector('.note-info')!.textContent).toContain('反向链接：无');
  expect(document.querySelector('.note-info')!.textContent).not.toContain('存储位置');
  document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); expect(document.querySelector('.note-info')).toBeNull();
  await open('显示简介'); await vi.waitFor(() => expect(document.querySelector('.note-info')).not.toBeNull());
  document.body.dispatchEvent(new Event('pointerdown', { bubbles: true })); expect(document.querySelector('.note-info')).toBeNull();
  expect(error).not.toHaveBeenCalled();
});
