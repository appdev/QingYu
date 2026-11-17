// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { setupFileWatch } from '../src/file-watch';
import type { Notebook } from '../src/notebook';

const handlers = vi.hoisted(() => new Map<string, (event: { payload: any }) => void>());
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async (name, handler) => { handlers.set(name, handler); return () => handlers.delete(name); }) }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => undefined) }));
let dispose: (() => void) | undefined;
beforeEach(() => { vi.useFakeTimers(); vi.clearAllMocks(); handlers.clear(); document.body.innerHTML = '<main id="app-shell"></main>'; document.body.inert = false; delete document.body.dataset.libraryBusy; delete document.body.dataset.syncBusy; });
afterEach(() => { dispose?.(); dispose = undefined; vi.useRealTimers(); });
async function fixture() {
  const note = { id: 'a', source: 'original' };
  const book = { root: '/notes', current: note, reconcile: vi.fn(async () => ({ changed: false, backups: [] as string[] })) };
  const actions = { pending: vi.fn(() => false), buffer: vi.fn(() => ({ noteId: note.id, source: note.source })), refresh: vi.fn(), backedUp: vi.fn(), error: vi.fn() };
  dispose = await setupFileWatch(book as unknown as Notebook, actions);
  const event = (root = '/notes') => handlers.get('library-files-changed')!({ payload: root });
  return { book, actions, event };
}

it('coalesces bursts, ignores old roots, and leaves the editor untouched for own-save/no-op events', async () => {
  const { book, actions, event } = await fixture();
  expect(invoke).toHaveBeenCalledWith('library_watch');
  event('/previous-library'); await vi.advanceTimersByTimeAsync(200); expect(book.reconcile).not.toHaveBeenCalled();
  event(); event(); event(); await vi.advanceTimersByTimeAsync(150);
  expect(book.reconcile).toHaveBeenCalledExactlyOnceWith('/notes', { noteId: 'a', source: 'original' });
  expect(actions.refresh).not.toHaveBeenCalled(); expect(document.body.dataset.libraryBusy).toBe('false');
});

it('restores the active table cell and caret after an own-save notification', async () => {
  const { book, actions, event } = await fixture();
  const shell = document.querySelector<HTMLElement>('#app-shell')!;
  shell.innerHTML = '<table contenteditable="true"><tr><td tabindex="0">表格文字</td></tr></table>';
  const cell = shell.querySelector('td')!;
  const selection = document.getSelection()!;
  Object.defineProperty(shell, 'inert', { set(value: boolean) {
    if (value) { cell.blur(); selection.removeAllRanges(); }
  } });
  cell.focus();
  selection.collapse(cell.firstChild, 2);
  event(); await vi.advanceTimersByTimeAsync(150);
  expect(book.reconcile).toHaveBeenCalledOnce();
  expect(actions.refresh).not.toHaveBeenCalled();
  expect(document.activeElement).toBe(cell);
  expect(selection.anchorNode).toBe(cell.firstChild);
  expect(selection.anchorOffset).toBe(2);
});

it('does not take focus back from a control outside the library while a reconcile is running', async () => {
  const { book, event } = await fixture();
  const editor = document.querySelector('#app-shell')!.appendChild(document.createElement('textarea'));
  const outside = document.body.appendChild(document.createElement('button'));
  editor.focus();
  book.reconcile.mockImplementationOnce(async () => {
    outside.focus();
    return { changed: false, backups: [] };
  });
  event(); await vi.advanceTimersByTimeAsync(150);
  expect(document.activeElement).toBe(outside);
});

it('has one deferred recheck during pending saves, composition, folder operations or sync', async () => {
  const { book, actions, event } = await fixture();
  actions.pending.mockReturnValue(true);
  event(); await vi.advanceTimersByTimeAsync(150); expect(book.reconcile).not.toHaveBeenCalled();
  document.body.dataset.syncBusy = 'true'; actions.pending.mockReturnValue(false);
  await vi.advanceTimersByTimeAsync(1600); expect(book.reconcile).not.toHaveBeenCalled();
  document.body.dataset.syncBusy = 'false';
  await vi.advanceTimersByTimeAsync(1600); expect(book.reconcile).toHaveBeenCalledOnce();
});

it('refreshes the correct editor owner, reports backups, and retries failed reads without retaining a UI lock', async () => {
  const { book, actions, event } = await fixture();
  book.reconcile.mockRejectedValueOnce('temporarily locked');
  book.reconcile.mockResolvedValueOnce({ changed: true, backups: ['.miaoyan-conflicts/a.md'] });
  event(); await vi.advanceTimersByTimeAsync(150);
  expect(actions.error).toHaveBeenCalledWith('temporarily locked'); expect(document.body.dataset.libraryBusy).toBe('false');
  await vi.advanceTimersByTimeAsync(1600);
  expect(actions.refresh).toHaveBeenCalledWith(book.current); expect(actions.backedUp).toHaveBeenCalledWith(['.miaoyan-conflicts/a.md']);
  expect((document.querySelector('#app-shell') as HTMLElement).inert).toBe(false);
});

it('does not apply a delayed event after the library has changed', async () => {
  const { book, event } = await fixture(); event(); book.root = '/new-notes';
  await vi.advanceTimersByTimeAsync(150); expect(book.reconcile).not.toHaveBeenCalled();
});
