// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest';
import { setupSidebarResize } from '../src/sidebar-resize';
import type { SidebarLayout } from '../src/sidebar-layout';

afterEach(() => { document.body.replaceChildren(); localStorage.clear(); vi.unstubAllGlobals(); });

it('keeps saved widths when release CSS removes the initial default body margin', () => {
  vi.stubGlobal('innerWidth', 1120); vi.stubGlobal('devicePixelRatio', 2);
  localStorage.setItem('miaoyan-editor.sidebar-size.v1', JSON.stringify({ folders: 292, notes: 360, columns: 'all' }));
  document.body.innerHTML = '<div id="app-shell"><section><div id="folder-resizer"></div></section><section><div id="notes-resizer"></div></section></div>';
  const shell = document.querySelector<HTMLElement>('#app-shell')!;
  let clientWidth = 1104;
  Object.defineProperty(shell, 'clientWidth', { get: () => clientWidth });
  const error = vi.fn();
  const resizing = setupSidebarResize(shell, () => ({ columns: 'all', unfolding: false }), vi.fn(), error);
  resizing.update();
  clientWidth = 1120;
  resizing.update();
  expect(shell.style.getPropertyValue('--folder-width')).toBe('292px');
  expect(shell.style.getPropertyValue('--notes-width')).toBe('360px');
  expect(JSON.parse(localStorage.getItem('miaoyan-editor.sidebar-size.v1')!)).toEqual({ version: 2, folders: 292, notes: 360, columns: 'all' });
  expect(error).not.toHaveBeenCalled();
});

it('starts external directory windows with folders collapsed without changing the library layout', () => {
  vi.stubGlobal('innerWidth', 1120); vi.stubGlobal('devicePixelRatio', 2);
  const library = JSON.stringify({ folders: 200, notes: 300, columns: 'all' });
  localStorage.setItem('miaoyan-editor.sidebar-size.v1', library);
  localStorage.setItem('miaoyan-editor.sidebar-size.external.v1', JSON.stringify({ folders: 280, notes: 340, columns: 'all' }));
  document.body.innerHTML = '<div id="app-shell"><section><div id="folder-resizer"></div></section><section><div id="notes-resizer"></div></section></div>';
  const shell = document.querySelector<HTMLElement>('#app-shell')!;
  let layout = { columns: 'list' as 'all' | 'list' | 'note', unfolding: false };
  const resizing = setupSidebarResize(shell, () => layout, vi.fn(), vi.fn(), true);
  expect(resizing.columns).toBe('list');
  resizing.update();
  expect(shell.style.getPropertyValue('--folder-width')).toBe('0px');
  expect(shell.style.getPropertyValue('--notes-width')).toBe('340px');
  layout = { columns: 'all', unfolding: false };
  resizing.update();
  expect(shell.style.getPropertyValue('--folder-width')).toBe('280px');
  expect(localStorage.getItem('miaoyan-editor.sidebar-size.v1')).toBe(library);
});

it('keeps the folders collapsed until the captured drag ends and retains remembered widths', () => {
  const { shell, move, pointer, apply, layout, captured } = startDrag('folders');
  for (const x of [121, 119, 292]) move(x);
  expect(layout().columns).toBe('list');
  expect(captured()).toBe(true);
  pointer(shell, 'pointerup', 500);
  expect(captured()).toBe(false);
  expect(JSON.parse(localStorage.getItem('miaoyan-editor.sidebar-size.v1')!)).toEqual({ version: 2, folders: 292, notes: 360, columns: 'list' });
  expect(shell.style.getPropertyValue('--folder-width')).toBe('0px');
  expect(shell.style.getPropertyValue('--notes-width')).toBe('360px');
  apply({ columns: 'all', unfolding: false });
  expect(shell.style.getPropertyValue('--folder-width')).toBe('292px');
  expect(shell.style.getPropertyValue('--notes-width')).toBe('360px');
});

it('can cancel a collapsed drag without overwriting its widths or layout', () => {
  const { shell, move, layout, captured } = startDrag('folders');
  move(100);
  document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
  expect(layout().columns).toBe('all');
  expect(shell.style.getPropertyValue('--folder-width')).toBe('292px');
  expect(shell.style.getPropertyValue('--notes-width')).toBe('360px');
  expect(captured()).toBe(false);
  expect(JSON.parse(localStorage.getItem('miaoyan-editor.sidebar-size.v1')!)).toEqual({ version: 2, folders: 292, notes: 360, columns: 'all' });
});

it('keeps the note list visible and shrinks the folder column once the list reaches its minimum', () => {
  const { shell, move, pointer, layout } = startDrag('notes');
  move(600);
  expect(shell.style.getPropertyValue('--folder-width')).toBe('270px');
  expect(shell.style.getPropertyValue('--notes-width')).toBe('330px');
  move(271);
  expect(layout().columns).toBe('all');
  expect(shell.style.getPropertyValue('--folder-width')).toBe('248px');
  expect(shell.style.getPropertyValue('--notes-width')).toBe('330px');
  pointer(shell, 'pointerup', 271);
  expect(JSON.parse(localStorage.getItem('miaoyan-editor.sidebar-size.v1')!)).toEqual({ version: 2, folders: 248, notes: 330, columns: 'all' });
});

function startDrag(kind: 'folders' | 'notes') {
  vi.stubGlobal('innerWidth', 1120); vi.stubGlobal('devicePixelRatio', 2);
  localStorage.setItem('miaoyan-editor.sidebar-size.v1', JSON.stringify({ folders: 292, notes: 360, columns: 'all' }));
  document.body.innerHTML = '<div id="app-shell"><section><div id="folder-resizer"></div></section><section><div id="notes-resizer"></div></section></div>';
  const shell = document.querySelector<HTMLElement>('#app-shell')!;
  let captured = false, layout: SidebarLayout = { columns: 'all', unfolding: false };
  shell.setPointerCapture = () => { captured = true; };
  shell.hasPointerCapture = () => captured;
  shell.releasePointerCapture = () => { captured = false; };
  function apply(next: SidebarLayout) { layout = next; resizing.update(); }
  const resizing = setupSidebarResize(shell, () => layout, apply, message => { throw Error(message); });
  resizing.update();
  const handle = document.querySelector<HTMLElement>(kind === 'folders' ? '#folder-resizer' : '#notes-resizer')!;
  handle.parentElement!.getBoundingClientRect = () => ({ width: kind === 'folders' ? 292 : 360 } as DOMRect);
  function pointer(target: HTMLElement, type: string, x: number) {
    const event = new MouseEvent(type, { button: 0, clientX: x, bubbles: true, cancelable: true });
    Object.defineProperty(event, 'pointerId', { value: 1 });
    if (type === 'pointerdown') target.onpointerdown!.call(target, event as unknown as PointerEvent);
    else target.dispatchEvent(event);
  }
  pointer(handle, 'pointerdown', kind === 'folders' ? 292 : 652);
  return { shell, pointer, move: (x: number) => pointer(shell, 'pointermove', x), apply, layout: () => layout, captured: () => captured };
}
