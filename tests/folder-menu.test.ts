// @vitest-environment jsdom
import { expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { folderMenuItems } from '../src/folder-menu';
import { Notebook } from '../src/notebook';
import { createSidebar } from '../src/sidebar';

it('ports SidebarProjectView validation: blank disables, clicked rows hide, roots stay protected', () => {
  const labels = (type: 'all' | 'folder' | 'root' | 'trash', clicked: boolean) => folderMenuItems(type, clicked);
  const blank = labels('all', false);
  expect(blank.filter(item => item.text).map(item => item.text)).toEqual(['新建文件夹', '新建子文件夹', '重命名文件夹', '移动到…', '在 Finder 中显示', '在终端中打开', '移到废纸篓']);
  expect(blank.filter(item => item.text && item.enabled).map(item => item.text)).toEqual(['在终端中打开']);
  expect(labels('all', true).map(item => item.text)).toEqual(['在终端中打开']);
  expect(labels('trash', true).map(item => item.text)).toEqual(['在 Finder 中显示']);
  expect(labels('folder', true).map(item => item.text)).toEqual(['新建子文件夹', '重命名文件夹', '移动到…', '在 Finder 中显示', '在终端中打开', '移到废纸篓']);
  expect(labels('root', true).map(item => item.text)).toEqual(['新建子文件夹', '在 Finder 中显示', '在终端中打开']);
});

it('returns to All and restores the current note after removing the selected empty folder', async () => {
  document.body.innerHTML = readFileSync('index.html', 'utf8').match(/<body[^>]*>([\s\S]*)<\/body>/)![1];
  vi.stubGlobal('CSS', { escape: (text: string) => text });
  const book = new Notebook({ getItem: () => null, setItem: () => undefined }, '# 原文');
  const folder = await book.createFolder('空文件夹');
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
  await vi.waitFor(() => expect(document.querySelector('#notes-heading')!.textContent).toBe('全部'));
  expect(showing).toBe(true); expect(book.current.source).toBe('# 原文'); expect(error).not.toHaveBeenCalled();
  document.body.replaceChildren(); vi.unstubAllGlobals();
});
