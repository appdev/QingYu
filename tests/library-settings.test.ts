// @vitest-environment jsdom
import { beforeEach, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { emitTo } from '@tauri-apps/api/event';
import { setupFileLibrary } from '../src/file-library-ui';
import type { Notebook } from '../src/notebook';
const handlers = vi.hoisted(() => new Map<string, (event: { payload: any }) => Promise<void>>());
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async (name, fn) => { handlers.set(name, fn); return () => {}; }), emitTo: vi.fn().mockResolvedValue(undefined) }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
beforeEach(() => { handlers.clear(); vi.clearAllMocks(); document.body.innerHTML = '<div id="app-shell"></div>'; delete document.body.dataset.syncBusy; delete document.body.dataset.libraryBusy; });

it('saves locally then exits immediately when sync fails, without a confirmation dialog', async () => {
  const actions = { save: vi.fn(async () => true), sync: vi.fn(async () => { throw new Error('offline'); }), refresh: vi.fn(), error: vi.fn() };
  await setupFileLibrary({ loadError: false } as Notebook, actions);
  await handlers.get('library-before-exit')!({ payload: null });
  expect(actions.save).toHaveBeenCalledOnce(); expect(actions.sync).toHaveBeenCalledOnce();
  expect(invoke).toHaveBeenCalledExactlyOnceWith('library_finish_exit'); expect(document.body.inert).toBe(false);
  expect(actions.error).not.toHaveBeenCalled();
});

it('bounds an unfinished sync on exit to three seconds without waiting on the sync busy flag', async () => {
  vi.useFakeTimers();
  try {
    const actions = { save: vi.fn(async () => true), sync: vi.fn(() => new Promise<void>(() => {})), refresh: vi.fn(), error: vi.fn() };
    await setupFileLibrary({ loadError: false } as Notebook, actions);
    document.body.dataset.syncBusy = 'true';
    const exit = handlers.get('library-before-exit')!({ payload: null });
    await vi.advanceTimersByTimeAsync(2999); expect(invoke).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1); await exit;
    expect(actions.save).toHaveBeenCalledOnce(); expect(invoke).toHaveBeenCalledExactlyOnceWith('library_finish_exit');
  } finally { document.body.inert = false; vi.useRealTimers(); }
});

it('waits for an external refresh before saving and completing a native exit', async () => {
  vi.useFakeTimers();
  try {
    const actions = { save: vi.fn(async () => true), refresh: vi.fn(), error: vi.fn() };
    await setupFileLibrary({ loadError: false } as Notebook, actions);
    document.body.dataset.libraryBusy = 'true';
    const exit = handlers.get('library-before-exit')!({ payload: null });
    expect(actions.save).not.toHaveBeenCalled();
    document.body.dataset.libraryBusy = 'false'; await vi.advanceTimersByTimeAsync(50); await exit;
    expect(actions.save).toHaveBeenCalledOnce(); expect(invoke).toHaveBeenCalledWith('library_finish_exit');
  } finally { document.body.inert = false; vi.useRealTimers(); }
});

it('reads the live note directory and flushes the editor before switching it', async () => {
  const order: string[] = [];
  const book = { root: '/old-notes', loadError: false, chooseRoot: vi.fn(async () => { order.push('choose'); book.root = '/new-notes'; }) };
  const actions = { save: vi.fn(async () => { order.push('save'); return true; }), refresh: vi.fn(), error: vi.fn() };
  await setupFileLibrary(book as unknown as Notebook, actions);
  await handlers.get('settings-library-request')!({ payload: { id: 'read', change: false } });
  expect(emitTo).toHaveBeenLastCalledWith('settings', 'settings-library-reply', { id: 'read', root: '/old-notes' }); expect(actions.save).not.toHaveBeenCalled();
  await handlers.get('settings-library-request')!({ payload: { id: 'choose', change: true } });
  expect(order).toEqual(['save', 'choose']); expect(actions.refresh).toHaveBeenCalledOnce();
  expect(emitTo).toHaveBeenLastCalledWith('settings', 'settings-library-reply', { id: 'choose', root: '/new-notes' }); expect(document.querySelector<HTMLElement>('#app-shell')!.inert).toBe(false);
});

it('blocks switching during sync or unfinished input and restores the editing lock after a cancelled picker', async () => {
  const book = { root: '/old-notes', loadError: false, chooseRoot: vi.fn().mockResolvedValue(undefined) };
  const actions = { save: vi.fn().mockResolvedValue(false), refresh: vi.fn(), error: vi.fn() };
  await setupFileLibrary(book as unknown as Notebook, actions);
  const request = () => handlers.get('settings-library-request')!({ payload: { id: 'choose', change: true } });
  document.body.dataset.syncBusy = 'true'; await request(); expect(actions.save).not.toHaveBeenCalled();
  document.body.dataset.syncBusy = 'false'; await request(); expect(book.chooseRoot).not.toHaveBeenCalled();
  expect(emitTo).toHaveBeenLastCalledWith('settings', 'settings-library-reply', { id: 'choose', error: '当前输入尚未保存，请完成输入后重试' });
  actions.save.mockResolvedValue(true); await request();
  expect(emitTo).toHaveBeenLastCalledWith('settings', 'settings-library-reply', { id: 'choose', root: '/old-notes' });
  expect(document.body.dataset.libraryBusy).toBe('false'); expect(document.querySelector<HTMLElement>('#app-shell')!.inert).toBe(false);
});
