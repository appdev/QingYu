// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest';
import { setupSidebarResize } from '../src/sidebar-resize';

afterEach(() => { document.body.replaceChildren(); localStorage.clear(); vi.unstubAllGlobals(); });

it('keeps saved widths when release CSS removes the initial default body margin', () => {
  vi.stubGlobal('innerWidth', 1120); vi.stubGlobal('devicePixelRatio', 2);
  localStorage.setItem('miaoyan-editor.sidebar-size.v1', JSON.stringify({ folders: 200, notes: 300, columns: 'all' }));
  document.body.innerHTML = '<div id="app-shell"><section><div id="folder-resizer"></div></section><section><div id="notes-resizer"></div></section></div>';
  const shell = document.querySelector<HTMLElement>('#app-shell')!;
  let clientWidth = 1104;
  Object.defineProperty(shell, 'clientWidth', { get: () => clientWidth });
  const error = vi.fn();
  const resizing = setupSidebarResize(shell, () => ({ columns: 'all', unfolding: false }), vi.fn(), error);
  resizing.update();
  clientWidth = 1120;
  resizing.update();
  expect(shell.style.getPropertyValue('--folder-width')).toBe('200px');
  expect(shell.style.getPropertyValue('--notes-width')).toBe('300px');
  expect(JSON.parse(localStorage.getItem('miaoyan-editor.sidebar-size.v1')!)).toEqual({ folders: 200, notes: 300, columns: 'all' });
  expect(error).not.toHaveBeenCalled();
});

it('starts external directory windows with folders collapsed without changing the library layout', () => {
  vi.stubGlobal('innerWidth', 1120); vi.stubGlobal('devicePixelRatio', 2);
  const library = JSON.stringify({ folders: 200, notes: 300, columns: 'all' });
  localStorage.setItem('miaoyan-editor.sidebar-size.v1', library);
  localStorage.setItem('miaoyan-editor.sidebar-size.external.v1', JSON.stringify({ folders: 180, notes: 320, columns: 'all' }));
  document.body.innerHTML = '<div id="app-shell"><section><div id="folder-resizer"></div></section><section><div id="notes-resizer"></div></section></div>';
  const shell = document.querySelector<HTMLElement>('#app-shell')!;
  let layout = { columns: 'list' as 'all' | 'list' | 'note', unfolding: false };
  const resizing = setupSidebarResize(shell, () => layout, vi.fn(), vi.fn(), true);
  expect(resizing.columns).toBe('list');
  resizing.update();
  expect(shell.style.getPropertyValue('--folder-width')).toBe('0px');
  expect(shell.style.getPropertyValue('--notes-width')).toBe('320px');
  layout = { columns: 'all', unfolding: false };
  resizing.update();
  expect(shell.style.getPropertyValue('--folder-width')).toBe('180px');
  expect(localStorage.getItem('miaoyan-editor.sidebar-size.v1')).toBe(library);
});
