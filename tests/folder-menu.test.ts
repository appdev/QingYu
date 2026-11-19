// @vitest-environment jsdom
import { expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { folderMenuItems } from '../src/folder-menu';
import { Notebook } from '../src/notebook';
import { createSidebar } from '../src/sidebar';
import { dismissPopupMenu } from '../src/popup-menu';

vi.hoisted(() => { Object.assign(globalThis, { matchMedia: () => ({ matches: false, addEventListener() {}, removeEventListener() {} }) }); });

it('groups creation and management for the clicked folder and protects the library root', () => {
  const labels = (type: 'all' | 'folder' | 'root' | 'trash', clicked: boolean) => folderMenuItems(type, clicked);
  expect(labels('all', false).map(item => item.text)).toEqual(['新建笔记', '新建文件夹', '', '在终端中打开']);
  expect(labels('trash', true).map(item => item.text)).toEqual(['在 Finder 中显示']);
  expect(labels('folder', true).map(item => item.text)).toEqual(['在新窗口打开', '', '新建笔记', '新建文件夹', '', '重命名文件夹', '复制文件夹', '移动到…', '', '上移', '下移', '', '在 Finder 中显示', '在终端中打开', '', '移到废纸篓']);
  expect(labels('root', true).find(item => item.text === '重命名文件夹')?.enabled).toBe(false);
  expect(labels('root', true).some(item => item.destructive || item.text === '移动到…')).toBe(false);
});

it('returns to All and restores the current note after removing the selected empty folder', async () => {
  document.body.innerHTML = readFileSync('index.html', 'utf8').match(/<body[^>]*>([\s\S]*)<\/body>/)![1];
  vi.stubGlobal('CSS', { escape: (text: string) => text });
  const book = new Notebook({ getItem: () => null, setItem: () => undefined }, '# 原文');
  const folder = await book.createFolder('空文件夹');
  vi.spyOn(book, 'root', 'get').mockReturnValue('/test-library');
  let showing = true;
  const error = vi.fn();
  createSidebar(() => book, {
    async select(id) { if (id) await book.select(id); showing = Boolean(id); return true; },
    async create() { return true; }, async save() { return true; }, reload() { showing = Boolean(book.current); }, error,
  });
  const row = () => document.querySelector<HTMLButtonElement>(`.folder-row[data-folder-id="${folder}"]`)!;
  row().click(); await vi.waitFor(() => expect(showing).toBe(false));
  vi.spyOn(book, 'folderAction').mockImplementation(async action => {
    expect(action).toBe('trash'); book.data = { ...book.data, folders: book.data.folders.filter(other => other.id !== folder) };
  });
  row().dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
  await vi.waitFor(() => expect(document.querySelector('[aria-label="移到废纸篓"]')).not.toBeNull());
  document.querySelector<HTMLButtonElement>('[aria-label="移到废纸篓"]')!.click();
  await vi.waitFor(() => expect(document.querySelector('#notes-heading')!.textContent).toBe('全部笔记'));
  expect(showing).toBe(true); expect(book.current.source).toBe('# 原文'); expect(error).not.toHaveBeenCalled();
  document.body.replaceChildren(); vi.unstubAllGlobals();
});

it('creates a note in the folder that was right-clicked without first changing the current list', async () => {
  document.body.innerHTML = readFileSync('index.html', 'utf8').match(/<body[^>]*>([\s\S]*)<\/body>/)![1];
  vi.stubGlobal('CSS', { escape: (text: string) => text });
  const book = new Notebook({ getItem: () => null, setItem: () => undefined }, '# 原文');
  const target = await book.createFolder('目标');
  const create = vi.fn(async () => true);
  const select = vi.fn(async () => true);
  createSidebar(() => book, { create, select, save: async () => true, reload() {}, error: vi.fn() });

  document.querySelector(`.folder-row[data-folder-id="${target}"]`)!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
  await vi.waitFor(() => expect(document.querySelector('[role=menuitem][aria-label="新建笔记"]')).not.toBeNull());
  expect(document.querySelectorAll('[role=menuitem][aria-label="在 Finder 中显示"], [role=menuitem][aria-label="在终端中打开"], [role=menuitem][aria-label="移到废纸篓"]')).toHaveLength(0);
  expect(select).not.toHaveBeenCalled();
  expect(document.querySelector('#notes-heading')!.textContent).toBe('全部笔记');
  document.querySelector<HTMLButtonElement>('[role=menuitem][aria-label="新建笔记"]')!.click();
  await vi.waitFor(() => expect(create).toHaveBeenCalledWith(target, ''));
  await vi.waitFor(() => expect(document.querySelector('#notes-heading')!.textContent).toBe('目标'));
  document.body.replaceChildren(); vi.unstubAllGlobals();
});

it('opens an icon menu and compact preview without selecting the long-pressed folder', async () => {
  document.body.innerHTML = readFileSync('index.html', 'utf8').match(/<body[^>]*>([\s\S]*)<\/body>/)![1];
  vi.stubGlobal('CSS', { escape: (text: string) => text });
  const { showModal } = HTMLDialogElement.prototype;
  HTMLDialogElement.prototype.showModal = function () { this.open = true; };
  try {
    const book = new Notebook({ getItem: () => null, setItem: () => undefined }, '# 原文');
    const id = await book.createFolder('空文件夹');
    vi.spyOn(book, 'root', 'get').mockReturnValue('/test-library');
    const select = vi.fn(async () => true);
    createSidebar(() => book, { select, create: async () => true, save: async () => true, reload() {}, error: vi.fn() }, true);

    const row = document.querySelector<HTMLButtonElement>(`.folder-row[data-folder-id="${id}"]`)!;
    row.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    const sheet = await vi.waitFor(() => {
      const dialog = document.querySelector('.context-sheet[open]');
      expect(dialog).not.toBeNull();
      return dialog!;
    });
    expect(sheet.querySelector('[data-folder-preview]')?.textContent).toBe('空文件夹0 篇笔记');
    expect(sheet.querySelector('.context-folder-preview')).toBeNull();
    const buttons = [...sheet.querySelectorAll('[role=menuitem]')];
    expect(buttons.map(button => button.ariaLabel)).toEqual(['新建笔记', '新建文件夹', '重命名文件夹', '复制文件夹', '移动到…', '移到废纸篓']);
    expect(buttons.every(button => button.querySelector('svg.context-action-icon'))).toBe(true);
    expect(buttons.find(button => button.ariaLabel === '复制文件夹')?.hasAttribute('disabled')).toBe(false);
    expect(buttons.find(button => button.ariaLabel === '移到废纸篓')?.hasAttribute('disabled')).toBe(false);
    expect(row.ariaPressed).toBe('false');
    expect(select).not.toHaveBeenCalled();
  } finally {
    dismissPopupMenu();
    HTMLDialogElement.prototype.showModal = showModal;
    document.body.replaceChildren();
    vi.unstubAllGlobals();
  }
});
