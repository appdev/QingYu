// @vitest-environment jsdom
import { readFileSync } from 'node:fs';
import { afterEach, expect, it, vi } from 'vitest';
import { Notebook } from '../src/notebook';
import { createSidebar } from '../src/sidebar';

vi.hoisted(() => { vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {} })); });

afterEach(() => vi.unstubAllGlobals());

it('restores an archived directory from the mobile Trash list', async () => {
  document.body.innerHTML = readFileSync('index.html', 'utf8').match(/<body[^>]*>([\s\S]*)<\/body>/)![1];
  vi.stubGlobal('matchMedia', () => Object.assign(new EventTarget(), { matches: false }));
  const book = new Notebook({ getItem: () => null, setItem() {} }, '# 笔记');
  book.trashedFolders = ['旧文件夹'];
  const restore = vi.spyOn(book, 'folderAction').mockImplementation(async () => { book.trashedFolders = []; });
  createSidebar(() => book, {
    select: async () => true, create: async () => true, save: async () => true, reload() {}, error: vi.fn(),
  }, true);
  document.querySelector<HTMLButtonElement>('.folder-row[data-trash=true]')!.click();
  const restoreButton = await vi.waitFor(() => {
    const button = document.querySelector<HTMLButtonElement>('[aria-label="恢复文件夹 旧文件夹"]');
    expect(button).not.toBeNull();
    return button!;
  });
  restoreButton.click();
  await vi.waitFor(() => expect(document.querySelector('.trash-folder-row')).toBeNull());
  expect(restore).toHaveBeenCalledWith('restore-folder', null, '旧文件夹');
  document.body.replaceChildren();
});

it('keeps the list and note when returning or changing tablet width', async () => {
  document.body.innerHTML = readFileSync('index.html', 'utf8').match(/<body[^>]*>([\s\S]*)<\/body>/)![1];
  history.replaceState(null, '');
  const tablet = Object.assign(new EventTarget(), { matches: false });
  vi.stubGlobal('matchMedia', () => tablet);
  vi.stubGlobal('CSS', { escape: (value: string) => value });
  const get = (selector: string) => document.querySelector<HTMLElement>(selector)!;
  const book = new Notebook({ getItem: () => null, setItem() {} }, '# 笔记');
  await book.createFolder('Alpha');
  await book.createFolder('Beta');
  const select = vi.fn(async () => true);
  const sidebar = createSidebar(() => book, {
    select, async create() { return true; }, async save() { return true; }, reload() {}, error: vi.fn(),
  }, true);
  const shell = get('#app-shell'), list = get('#notes-list'), editor = get('#document-column');
  const expectPage = (page: string) => vi.waitFor(() => expect(shell.dataset.mobilePage).toBe(page));
  expect(shell.dataset.mobilePage).toBe('folders');
  expect(get('#notes-pane > glass-progressive-blur.notes-scroll-edge')).not.toBeNull();
  expect(editor.inert).toBe(true);
  expect(get('.folder-toolbar #sidebar-account').querySelector('.sidebar-account-avatar svg')).not.toBeNull();
  expect(get('#settings-button').hidden).toBe(true);
  expect(get('.mobile-library-search').querySelector('svg + #note-search')).not.toBeNull();

  get('.folder-row').click();
  await expectPage('notes');
  expect(select).not.toHaveBeenCalled();
  list.scrollTop = 325;
  get('.note-row').click();
  await expectPage('note');
  expect(get('#notes-pane').inert).toBe(true);
  const consumed = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true });
  consumed.preventDefault();
  editor.dispatchEvent(consumed);
  expect(shell.dataset.mobilePage).toBe('note');
  sidebar.refresh();

  tablet.matches = true;
  tablet.dispatchEvent(new Event('change'));
  expect(shell.hasAttribute('data-mobile-wide')).toBe(true);
  expect(shell.hasAttribute('data-mobile-motion')).toBe(false);
  expect(get('#notes-pane').inert).toBe(false);
  expect(editor.inert).toBe(false);
  tablet.matches = false;
  tablet.dispatchEvent(new Event('change'));
  expect(shell.dataset.mobilePage).toBe('note');
  expect(get('#notes-pane').inert).toBe(true);

  get('#sidebar-button').click();
  await expectPage('notes');
  expect(get('#notes-list')).toBe(list);
  expect(list.scrollTop).toBe(325);
  get('#folders-back').click();
  await expectPage('folders');
  expect(select).toHaveBeenCalledTimes(1);

  get('#edit-folders').click();
  await vi.waitFor(() => expect(get('#folder-pane').hasAttribute('data-editing-folders')).toBe(true));
  expect(get('#edit-folders').ariaLabel).toBe('完成编辑');
  expect((get('.folder-row') as HTMLButtonElement).disabled).toBe(true);
  get('[aria-label="编辑文件夹 Alpha"]').click();
  await vi.waitFor(() => expect(get('[role=menuitem][aria-label="下移"]')).not.toBeNull());
  expect(get('[role=menuitem][aria-label="移到废纸篓"]')).toBeNull();
  get('[role=menuitem][aria-label="下移"]').click();
  await vi.waitFor(() => expect(get('.folder-entry .folder-name').textContent).toBe('Beta'));
  expect(shell.dataset.mobilePage).toBe('folders');
  get('#edit-folders').click();
  await vi.waitFor(() => expect(get('#edit-folders').textContent).toBe('编辑'));
  const search = get('#note-search');
  search.focus();
  await expectPage('notes');
  expect(document.activeElement).toBe(search);
});
