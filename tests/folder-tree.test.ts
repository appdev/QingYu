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

function renameField() {
  const input = document.querySelector<HTMLInputElement>('.rename-folder');
  if (!input) throw new Error('改名输入框还没出现');
  return input;
}

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

it('keeps a folder’s date grouping separate from All Notes and its subfolders', async () => {
  const { row, arrow } = fixture();
  const button = (selector: string) => document.querySelector<HTMLButtonElement>(selector)!;
  const headings = () => [...document.querySelectorAll('#notes-list h3')].map(item => item.textContent);
  const select = async (id: string) => {
    row(id).click();
    await vi.waitFor(() => expect(row(id).getAttribute('aria-pressed')).toBe('true'));
  };
  await select('parent');
  expect(headings()).toEqual(['今天']);
  button('#list-more-button').click();
  button('[role=menuitem][aria-label="按日期分组"]').click();
  button('[role=menuitemradio][aria-label="关闭"]').click();
  await vi.waitFor(() => expect(headings()).toEqual([]));

  await select('');
  expect(headings()).toEqual(['今天']);
  arrow('parent').click();
  await vi.waitFor(() => expect(row('child')).not.toBeNull());
  await select('child');
  expect(headings()).toEqual(['今天']);
  await select('parent');
  expect(headings()).toEqual([]);
});

it('creates an untitled folder under the right-clicked parent, selects it and names it in place, as Pinto does', async () => {
  const { book, row, sidebar, error } = fixture();
  row('parent').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
  await vi.waitFor(() => expect(document.querySelector('[role=menuitem][aria-label="新建文件夹"]')).not.toBeNull());
  document.querySelector<HTMLButtonElement>('[role=menuitem][aria-label="新建文件夹"]')!.click();
  const input = await vi.waitFor(renameField);
  expect(document.querySelector<HTMLDialogElement>('#folder-dialog')!.open).toBe(false);
  // 名称已经全选，直接打字就替换；所在的父文件夹自动展开，新文件夹处于选中状态。
  expect([input.value, input.selectionStart, input.selectionEnd]).toEqual(['新建文件夹', 0, 5]);
  expect(document.activeElement).toBe(input);
  const untitled = book.data.folders.find(folder => folder.path === '父目录/新建文件夹')!;
  expect(input.closest<HTMLElement>('.renaming')!.dataset.folderId).toBe(untitled.id);
  expect(document.querySelector('#notes-heading')!.textContent).toBe('新建文件夹');
  expect(row('parent').getAttribute('aria-expanded')).toBe('true');
  input.value = '新增';
  input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
  await vi.waitFor(() => expect(book.data.folders.find(folder => folder.id === untitled.id)?.path).toBe('父目录/新增'));
  const child = book.data.folders.find(folder => folder.path === '父目录/新增')!;
  await vi.waitFor(() => expect(row(child.id).getAttribute('aria-pressed')).toBe('true'));
  await book.folderAction('rename', 'parent', '改名'); sidebar.refresh();
  expect(row(child.id)).not.toBeNull(); expect(row('parent').getAttribute('aria-expanded')).toBe('true');
  const second = createSidebar(() => book, { async select() { return true; }, async create() { return true; }, async save() { return true; }, reload() {}, error });
  second.refresh(); expect(row(child.id)).not.toBeNull();
  expect(error).not.toHaveBeenCalled();
});

it('numbers another untitled folder and keeps the name on Escape', async () => {
  const { book, row, error } = fixture();
  book.data.folders.push({ id: 'taken', name: '新建文件夹', path: '新建文件夹' });
  document.querySelector<HTMLButtonElement>('#new-folder-button')!.click();
  const input = await vi.waitFor(renameField);
  expect(input.value).toBe('新建文件夹 2');
  input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
  const created = book.data.folders.find(folder => folder.path === '新建文件夹 2')!;
  await vi.waitFor(() => expect(row(created.id).getAttribute('aria-pressed')).toBe('true'));
  expect(document.querySelector('.rename-folder')).toBeNull();
  expect(error).not.toHaveBeenCalled();
});

it('keeps naming a new folder while the file watcher reloads the library', async () => {
  const { book, sidebar } = fixture();
  document.querySelector<HTMLButtonElement>('#new-folder-button')!.click();
  const input = await vi.waitFor(renameField);
  // 核对笔记库时界面临时 inert，WebKit 会让输入框失焦，随后侧栏刷新。
  const shell = document.querySelector<HTMLElement>('#app-shell')!;
  shell.setAttribute('inert', '');
  input.dispatchEvent(new FocusEvent('blur'));
  sidebar.refresh();
  shell.removeAttribute('inert');
  expect(document.querySelector('.rename-folder')).toBe(input);
  input.value = '读书';
  input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
  await vi.waitFor(() => expect(book.data.folders.some(folder => folder.path === '读书')).toBe(true));
  expect(book.data.folders.some(folder => folder.path === '新建文件夹')).toBe(false);
});

it('puts a ••• button in place of the count that opens the folder’s own menu', async () => {
  const { row } = fixture();
  const entry = row('peer').closest('.folder-entry')!;
  const more = entry.querySelector<HTMLButtonElement>('.folder-row-more')!;
  expect(more).not.toBeNull();
  // 和 Pinto 一样不进入键盘焦点顺序，读屏走右键菜单。
  expect([more.tabIndex, more.ariaHidden, more.title]).toEqual([-1, 'true', '文件夹选项']);
  expect(row('').closest('.folder-entry')).toBeNull();
  more.click();
  await vi.waitFor(() => expect(document.querySelector('[role=menuitem][aria-label="重命名文件夹"]')).not.toBeNull());
  document.querySelector<HTMLButtonElement>('[role=menuitem][aria-label="重命名文件夹"]')!.click();
  await vi.waitFor(() => expect(document.querySelector<HTMLElement>('.renaming')?.dataset.folderId).toBe('peer'));
});

it('moves only among siblings, keeps children attached and restores the chosen order on reload', async () => {
  const { book, row, arrow } = fixture();
  const order = () => [...document.querySelectorAll<HTMLElement>('.folder-entry .folder-row')].map(row => row.dataset.folderId);
  const [first, second] = order();
  arrow('parent').click(); await vi.waitFor(() => expect(row('child')).not.toBeNull());
  row(second!).dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
  await vi.waitFor(() => expect(document.querySelector<HTMLButtonElement>('[aria-label="下移"]')?.disabled).toBe(true));
  document.querySelector<HTMLButtonElement>('[aria-label="上移"]')!.click();
  await vi.waitFor(() => expect(order().filter(id => id !== 'child')).toEqual([second, first]));
  expect(order()[order().indexOf('parent') + 1]).toBe('child');
  expect(book.data.folders.find(folder => folder.id === 'child')!.path).toBe('父目录/子目录');
  document.body.innerHTML = readFileSync('index.html', 'utf8').match(/<body[^>]*>([\s\S]*)<\/body>/)![1];
  createSidebar(() => book, { async select() { return true; }, async create() { return true; }, async save() { return true; }, reload() {}, error: vi.fn() });
  expect(order().filter(id => id !== 'child')).toEqual([second, first]);
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

it('opens the list options as the same glass toolbar menu as the note menu and closes it on a second click', async () => {
  fixture();
  const more = document.querySelector<HTMLButtonElement>('#list-more-button')!;
  more.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, detail: 1 }));
  await vi.waitFor(() => expect(document.querySelector('[role=menu][aria-label="列表选项"]')).not.toBeNull());
  const menu = document.querySelector<HTMLElement>('[role=menu][aria-label="列表选项"]')!;
  expect(menu.classList.contains('toolbar-context-menu')).toBe(true);
  expect(menu.classList.contains('desktop-toolbar-menu')).toBe(true);
  expect(more.getAttribute('aria-expanded')).toBe('true');
  more.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true, detail: 1 }));
  await vi.waitFor(() => expect(document.querySelector('[role=menu][aria-label="列表选项"]')).toBeNull());
  expect(more.getAttribute('aria-expanded')).toBe('false');
});
