// @vitest-environment jsdom
import { readFileSync } from 'node:fs';
import { afterEach, expect, it, vi } from 'vitest';
import { Notebook } from '../src/notebook';
import { createSidebar } from '../src/sidebar';

afterEach(() => vi.unstubAllGlobals());

it('keeps the list and note when returning or changing tablet width', async () => {
  document.body.innerHTML = readFileSync('index.html', 'utf8').match(/<body[^>]*>([\s\S]*)<\/body>/)![1];
  history.replaceState(null, '');
  const tablet = Object.assign(new EventTarget(), { matches: false });
  vi.stubGlobal('matchMedia', () => tablet);
  const get = (selector: string) => document.querySelector<HTMLElement>(selector)!;
  const book = new Notebook({ getItem: () => null, setItem() {} }, '# 笔记');
  const select = vi.fn(async () => true);
  const sidebar = createSidebar(() => book, {
    select, async create() { return true; }, async save() { return true; }, reload() {}, error: vi.fn(),
  }, true);
  const shell = get('#app-shell'), list = get('#notes-list'), editor = get('#document-column');
  const expectPage = (page: string) => vi.waitFor(() => expect(shell.dataset.mobilePage).toBe(page));
  expect(shell.dataset.mobilePage).toBe('folders');
  expect(editor.inert).toBe(true);

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
});
