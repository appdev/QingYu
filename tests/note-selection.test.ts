// @vitest-environment jsdom
import './fixtures/chinese-ui';
import { readFileSync } from 'node:fs';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { Notebook } from '../src/notebook';
import { createSidebar } from '../src/sidebar';
import { dismissPopupMenu } from '../src/popup-menu';

vi.hoisted(() => { vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {} })); });

beforeEach(() => {
  document.body.innerHTML = readFileSync('index.html', 'utf8').match(/<body[^>]*>([\s\S]*)<\/body>/)![1];
  localStorage.clear();
  history.replaceState(null, '');
  vi.stubGlobal('CSS', { escape: (value: string) => value });
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {} }));
});
afterEach(() => { dismissPopupMenu(); document.body.replaceChildren(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

function fixture(mobile = false) {
  const book = new Notebook({ getItem: () => null, setItem() {} }, '# First');
  const initial = book.current;
  const notes = Array.from({ length: 5 }, (_, index) => ({
    ...initial, id: `${initial.id}-${index}`, title: `Note ${index}`, source: `# Note ${index}`,
    updatedAt: Date.now() - index * 10 * 864e5,
  }));
  book.data.notes = notes;
  book.data.activeId = notes[0].id;
  book.data.folders.push({ id: 'target', name: '目标目录' });
  const save = vi.fn(async () => true), error = vi.fn();
  const select = vi.fn(async (id: string | null) => { if (!await save()) return false; if (id) await book.select(id); return true; });
  const sidebar = createSidebar(() => book, { select, save, error, async create() { return true; }, reload() {} }, mobile);
  const row = (index: number) => document.querySelector<HTMLButtonElement>(`.note-row[data-note-id="${notes[index].id}"]`)!;
  const selected = () => [...document.querySelectorAll<HTMLElement>('.note-row[aria-selected=true]')].map(row => row.dataset.noteId);
  const click = (index: number, init: MouseEventInit = {}) => row(index).dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, ...init }));
  const menu = (index: number) => row(index).dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
  const item = (label: string) => document.querySelector<HTMLButtonElement>(`[role=menuitem][aria-label="${label}"]`)!;
  return { book, notes, sidebar, save, error, select, row, selected, click, menu, item };
}

it('selects and shrinks a Shift range across date groups without opening another note', async () => {
  const { notes, sidebar, selected, click, select } = fixture();
  expect(document.querySelector('#notes-list')!.getAttribute('aria-multiselectable')).toBe('true');
  click(3, { shiftKey: true });
  expect(selected()).toEqual(notes.slice(0, 4).map(note => note.id));
  sidebar.refresh();
  expect(selected()).toHaveLength(4);
  click(1, { shiftKey: true });
  expect(selected()).toEqual(notes.slice(0, 2).map(note => note.id));
  expect(select).not.toHaveBeenCalled();
});

it.each(['metaKey', 'ctrlKey'])('toggles separate notes with %s and plain click returns to one note', async modifier => {
  const { notes, selected, click, select } = fixture();
  click(3, { [modifier]: true });
  expect(selected()).toEqual([notes[0].id, notes[3].id]);
  click(0, { [modifier]: true });
  expect(selected()).toEqual([notes[3].id]);
  click(3, { [modifier]: true });
  expect(selected()).toEqual([]);
  click(2);
  await vi.waitFor(() => expect(selected()).toEqual([notes[2].id]));
  expect(select).toHaveBeenCalledOnce();
});

it('keeps the range when opening a different note fails to save', async () => {
  const { click, save, selected } = fixture();
  click(2, { shiftKey: true });
  save.mockResolvedValue(false);
  click(4);
  await vi.waitFor(() => expect(save).toHaveBeenCalled());
  expect(selected()).toHaveLength(3);
});

it('clears hidden selections on search and folder changes, and follows the visible sort order', async () => {
  const { notes, book, sidebar, click, selected } = fixture();
  click(3, { shiftKey: true });
  sidebar.searchAll('Note 2');
  expect(selected()).toEqual([]);
  click(2, { shiftKey: true });
  expect(selected()).toEqual([notes[2].id]);
  sidebar.searchAll('');
  expect(selected()).toEqual([notes[0].id]);
  book.data.notes[4].pinned = true;
  sidebar.refresh();
  click(4, { shiftKey: true });
  expect(selected()).toEqual([notes[4].id, notes[0].id]);
  document.querySelector<HTMLButtonElement>('.folder-row[data-folder-id="target"]')!.click();
  await vi.waitFor(() => expect(document.querySelector('#notes-list .note-row')).toBeNull());
  expect(selected()).toEqual([]);
});

it('extends the selection with Shift and arrows and opens its menu from the keyboard', async () => {
  const { notes, row, selected, item } = fixture();
  row(0).focus();
  row(0).dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', shiftKey: true, bubbles: true, cancelable: true }));
  expect(selected()).toEqual(notes.slice(0, 2).map(note => note.id));
  expect(document.activeElement).toBe(row(1));
  row(1).dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true, bubbles: true, cancelable: true }));
  await vi.waitFor(() => expect(item('移动 2 篇笔记到…')).not.toBeNull());
  expect(item('删除 2 篇笔记')).not.toBeNull();
});

it('moves the selected notes using the existing folder submenu and leaves other notes intact', async () => {
  const { book, notes, click, menu, item, error } = fixture();
  book.data.notes[1].folderId = 'target';
  const move = vi.spyOn(book, 'moveNote');
  click(2, { shiftKey: true });
  menu(1);
  await vi.waitFor(() => expect(item('移动 3 篇笔记到…')).not.toBeNull());
  item('移动 3 篇笔记到…').click();
  item('目标目录').click();
  await vi.waitFor(() => expect(book.data.notes.slice(0, 3).every(note => note.folderId === 'target')).toBe(true));
  expect(move.mock.calls.map(([id]) => id)).toEqual([notes[0].id, notes[2].id]);
  expect(book.data.notes[3].folderId).toBe('default');
  expect(book.data.notes.map(note => note.source)).toEqual(notes.map(note => note.source));
  expect(error).not.toHaveBeenCalled();
});

it('only trashes the selected notes and respects a failed save', async () => {
  const { book, click, menu, item, save } = fixture();
  click(2, { shiftKey: true });
  menu(1);
  await vi.waitFor(() => expect(item('删除 3 篇笔记')).not.toBeNull());
  save.mockResolvedValue(false);
  item('删除 3 篇笔记').click();
  await vi.waitFor(() => expect(document.querySelector('.note-context-menu')).toBeNull());
  expect(book.data.notes.some(note => note.trashed)).toBe(false);
  save.mockResolvedValue(true);
  menu(1);
  await vi.waitFor(() => expect(item('删除 3 篇笔记')).not.toBeNull());
  item('删除 3 篇笔记').click();
  await vi.waitFor(() => expect(book.data.notes.filter(note => note.trashed)).toHaveLength(3));
  expect(book.data.notes.slice(3).every(note => !note.trashed)).toBe(true);
});

it('reports a partial batch failure and refreshes the rows to match saved data', async () => {
  const { book, notes, click, menu, item, error, row } = fixture();
  const trash = book.trashNote.bind(book);
  vi.spyOn(book, 'trashNote').mockImplementation((id, trashed) => id === notes[1].id ? Promise.reject(new Error('磁盘已满')) : trash(id, trashed));
  click(2, { shiftKey: true });
  menu(1);
  await vi.waitFor(() => expect(item('删除 3 篇笔记')).not.toBeNull());
  item('删除 3 篇笔记').click();
  await vi.waitFor(() => expect(error).toHaveBeenCalledWith(expect.stringContaining('已完成 1/3 篇')));
  expect(book.data.notes.map(note => Boolean(note.trashed))).toEqual([true, false, false, false, false]);
  expect(row(0)).toBeNull();
  expect(document.body.dataset.libraryBusy).toBe('false');
});

it('right clicking outside the selection returns to a single-note menu', async () => {
  const { notes, selected, click, menu, item } = fixture();
  click(2, { shiftKey: true });
  menu(4);
  await vi.waitFor(() => expect(item('移到废纸篓')).not.toBeNull());
  expect(selected()).toEqual([notes[4].id]);
  expect(item('删除 3 篇笔记')).toBeNull();
});

it('pins and unpins the same note on consecutive shortcut presses', async () => {
  const { book, row } = fixture();
  const shell = document.querySelector<HTMLElement>('#app-shell')!;
  const focus = HTMLElement.prototype.focus;
  // jsdom allows focusing descendants of an inert element.
  vi.spyOn(HTMLElement.prototype, 'focus').mockImplementation(function (this: HTMLElement, options) {
    if (!shell.inert || !shell.contains(this)) focus.call(this, options);
  });
  const pressPin = () => document.activeElement!.dispatchEvent(new KeyboardEvent('keydown', {
    key: 'P', metaKey: true, shiftKey: true, bubbles: true, cancelable: true,
  }));
  row(0).focus();
  pressPin();
  await vi.waitFor(() => expect(book.current.pinned).toBe(true));
  expect(document.activeElement).toBe(row(0));
  pressPin();
  await vi.waitFor(() => expect(book.current.pinned).toBe(false));
});

it('uses the selected range for the existing delete shortcut', async () => {
  const { book, click, row } = fixture();
  click(2, { shiftKey: true });
  row(2).focus();
  row(2).dispatchEvent(new KeyboardEvent('keydown', { key: 'Backspace', metaKey: true, bubbles: true, cancelable: true }));
  await vi.waitFor(() => expect(book.data.notes.filter(note => note.trashed)).toHaveLength(3));
});

it('restores a range from the trash without changing the remaining trashed note', async () => {
  const { book, sidebar, click, menu, item, row } = fixture();
  for (const note of book.data.notes.slice(0, 3)) note.trashed = true;
  sidebar.refresh();
  document.querySelector<HTMLButtonElement>('.folder-row[data-trash=true]')!.click();
  await vi.waitFor(() => expect(row(0)).not.toBeNull());
  click(1, { shiftKey: true });
  menu(1);
  await vi.waitFor(() => expect(item('恢复 2 篇笔记')).not.toBeNull());
  item('恢复 2 篇笔记').click();
  await vi.waitFor(() => expect(book.data.notes.filter(note => note.trashed)).toHaveLength(1));
  expect(book.data.notes[2].trashed).toBe(true);
});

it('cancels a batch if the library changes while the draft is being saved', async () => {
  const { book, click, menu, item, save, error } = fixture();
  let root = '/original';
  vi.spyOn(book, 'root', 'get').mockImplementation(() => root);
  click(2, { shiftKey: true });
  menu(1);
  await vi.waitFor(() => expect(item('删除 3 篇笔记')).not.toBeNull());
  save.mockImplementation(async () => { root = '/other'; return true; });
  item('删除 3 篇笔记').click();
  await vi.waitFor(() => expect(error).toHaveBeenCalledWith(expect.stringContaining('笔记库已切换')));
  expect(book.data.notes.some(note => note.trashed)).toBe(false);
});

it('keeps a mobile list open when Shift selects notes with a hardware keyboard', async () => {
  const { click, selected, menu, item } = fixture(true);
  document.querySelector<HTMLButtonElement>('.folder-row')!.click();
  await vi.waitFor(() => expect(document.querySelector<HTMLElement>('#app-shell')!.dataset.mobilePage).toBe('notes'));
  click(2, { shiftKey: true });
  expect(selected()).toHaveLength(3);
  expect(document.querySelector<HTMLElement>('#app-shell')!.dataset.mobilePage).toBe('notes');
  menu(1);
  await vi.waitFor(() => expect(item('删除 3 篇笔记')).not.toBeNull());
  expect(document.querySelector('.context-preview')).toBeNull();
});
