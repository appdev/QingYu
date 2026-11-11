// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { folderRows } from '../src/folder-tree';
import { Notebook, visibleNotes } from '../src/notebook';
import { createSidebar } from '../src/sidebar';

beforeEach(() => {
  document.body.innerHTML = readFileSync('index.html', 'utf8').match(/<body[^>]*>([\s\S]*)<\/body>/)![1];
  localStorage.clear(); vi.stubGlobal('CSS', { escape: (text: string) => text });
  HTMLDialogElement.prototype.showModal = function () { this.open = true; };
  HTMLDialogElement.prototype.close = function () { this.open = false; };
});
afterEach(() => { document.body.replaceChildren(); vi.unstubAllGlobals(); });

function fixture() {
  const book = new Notebook({ getItem: () => null, setItem: () => undefined }, '# 根笔记');
  book.data.folders.push({ id: 'parent', name: '父目录', path: '父目录' }, { id: 'child', name: '子目录', path: '父目录/子目录' }, { id: 'peer', name: '同级', path: '同级' });
  const note = book.current;
  book.data.notes.push({ ...note, id: 'nested', folderId: 'child', title: '子笔记', source: '# 子笔记' });
  const error = vi.fn();
  const sidebar = createSidebar(() => book, { async select(id) { if (id) await book.select(id); return true; }, async create() { return true; }, async save() { return true; }, reload() {}, error });
  const row = (id: string) => document.querySelector<HTMLButtonElement>(`.folder-row[data-folder-id="${id}"]`)!;
  const arrow = (id: string) => row(id).closest('.folder-entry')!.querySelector<HTMLButtonElement>('.folder-disclosure')!;
  return { book, sidebar, row, arrow, error };
}

it('models immediate children in depth-first sibling order, hides the library root and starts collapsed', () => {
  const folders = [{ id: 'default', name: '备忘录', path: '' }, { id: 'a', name: 'A', path: 'A' }, { id: 'child', name: 'Z', path: 'A/Z' }, { id: 'peer', name: 'A-other', path: 'A-other' }];
  expect(folderRows(folders, new Set()).map(row => row.folder.id)).toEqual(['a', 'peer']);
  expect(folderRows(folders, new Set(['a'])).map(row => [row.folder.id, row.depth, row.parentId, row.hasChildren])).toEqual([
    ['a', 0, null, true], ['child', 1, 'a', false], ['peer', 0, null, false],
  ]);
});

it('includes descendants when selecting a parent and collapses a selected descendant back to its parent', async () => {
  const { book, row, arrow } = fixture();
  expect(row('default')).toBeNull(); expect(row('child')).toBeNull();
  expect(visibleNotes(book.data, 'parent', '').map(note => note.id)).toEqual(['nested']);
  arrow('parent').click(); await vi.waitFor(() => expect(row('child')).not.toBeNull());
  expect(row('child').style.getPropertyValue('--folder-depth')).toBe('1');
  row('child').click(); await vi.waitFor(() => expect(row('child').getAttribute('aria-pressed')).toBe('true'));
  arrow('parent').click(); await vi.waitFor(() => expect(row('child')).toBeNull());
  expect(row('parent').getAttribute('aria-pressed')).toBe('true'); expect(book.current.id).toBe('nested');
});

it('creates under the right-clicked parent without changing selection or forcing a collapsed parent open', async () => {
  const { book, row, arrow, sidebar, error } = fixture();
  arrow('parent').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
  await vi.waitFor(() => expect(document.querySelector('[aria-label="新建子文件夹"]')).not.toBeNull());
  document.querySelector<HTMLButtonElement>('[aria-label="新建子文件夹"]')!.click();
  await vi.waitFor(() => expect(document.querySelector<HTMLDialogElement>('#folder-dialog')!.open).toBe(true));
  document.querySelector<HTMLInputElement>('#folder-name')!.value = '新增';
  document.querySelector('#folder-form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
  await vi.waitFor(() => expect(document.querySelector<HTMLDialogElement>('#folder-dialog')!.open).toBe(false));
  const child = book.data.folders.find(folder => folder.path === '父目录/新增')!;
  expect(child).toBeDefined(); expect(row('parent').getAttribute('aria-pressed')).toBe('true'); expect(row(child.id)).toBeNull();
  arrow('parent').click(); await vi.waitFor(() => expect(row(child.id)).not.toBeNull());
  await book.folderAction('rename', 'parent', '改名'); sidebar.refresh();
  expect(row(child.id)).not.toBeNull(); expect(row('parent').getAttribute('aria-expanded')).toBe('true');
  const second = createSidebar(() => book, { async select() { return true; }, async create() { return true; }, async save() { return true; }, reload() {}, error });
  second.refresh(); expect(row(child.id)).not.toBeNull();
  expect(error).not.toHaveBeenCalled();
});

it('uses outline arrow keys and commits an inline rename before collapsing its branch', async () => {
  const { book, row, arrow, error } = fixture();
  row('parent').click(); await vi.waitFor(() => expect(row('parent').getAttribute('aria-pressed')).toBe('true'));
  row('parent').dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
  await vi.waitFor(() => expect(row('child')).not.toBeNull());
  row('child').click(); await vi.waitFor(() => expect(document.activeElement).toBe(row('child')));
  row('child').dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowLeft', bubbles: true }));
  await vi.waitFor(() => expect(row('parent').getAttribute('aria-pressed')).toBe('true'));
  row('parent').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
  await vi.waitFor(() => expect(document.querySelector('[aria-label="重命名文件夹"]')).not.toBeNull());
  document.querySelector<HTMLButtonElement>('[aria-label="重命名文件夹"]')!.click();
  await vi.waitFor(() => expect(document.querySelector('.rename-folder')).not.toBeNull());
  const input = document.querySelector<HTMLInputElement>('.rename-folder')!; input.value = '已提交';
  arrow('parent').click();
  await vi.waitFor(() => expect(book.data.folders.find(folder => folder.id === 'parent')!.name).toBe('已提交'));
  await vi.waitFor(() => expect(row('child')).toBeNull());
  expect(book.data.folders.find(folder => folder.id === 'child')!.path).toBe('已提交/子目录');
  expect(error).not.toHaveBeenCalled();
});

it('keeps an externally moved selected folder visible under its new ancestors without clearing search', async () => {
  const { book, row, arrow, sidebar } = fixture();
  arrow('parent').click(); await vi.waitFor(() => expect(row('child')).not.toBeNull());
  row('child').click(); await vi.waitFor(() => expect(row('child').getAttribute('aria-pressed')).toBe('true'));
  const search = document.querySelector<HTMLInputElement>('#note-search')!; search.value = '子笔记';
  book.data.folders.find(f => f.id === 'parent')!.path = '同级/父目录';
  book.data.folders.find(f => f.id === 'child')!.path = '同级/父目录/子目录';
  sidebar.refresh();
  expect(row('child')).not.toBeNull(); expect(row('child').getAttribute('aria-pressed')).toBe('true');
  expect(search.value).toBe('子笔记'); expect(book.current.id).toBe('nested');
});
