// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { Notebook, type Library } from '../src/notebook';
import { setupWorkspace } from '../src/workspace';

const handlers = vi.hoisted(() => new Map<string, (event: { payload: any }) => any>());
const listeners = vi.hoisted(() => [] as { name: string; handler: (event: { payload: any }) => any; target?: string }[]);
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async (name, handler, options) => { handlers.set(name, handler); listeners.push({ name, handler, target: options?.target }); return () => handlers.delete(name); }) }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
const disk: Library = { version: 1, activeId: 'external', folders: [{ id: 'default', name: 'Downloads', path: '' }], notes: [{ id: 'external', folderId: 'default', path: 'file.md', filename: 'file.md', title: 'file', source: 'original', createdAt: 1, updatedAt: 1 }] };
let requests: string[], paths: string[];
beforeEach(() => {
  vi.clearAllMocks(); handlers.clear(); listeners.length = 0; requests = []; paths = [];
  document.body.innerHTML = '<div id="app-shell"><button id="workspace-back" hidden></button></div>';
  document.body.inert = false; delete document.body.dataset.libraryBusy; delete document.body.dataset.syncBusy;
  vi.mocked(invoke).mockImplementation(async command => {
    if (command === 'workspace_requests') return requests.splice(0);
    if (command === 'workspace_paths') return paths.splice(0);
    if (command === 'workspace_open') return 'document-a';
    if (command === 'workspace_ready') return null;
  });
});
afterEach(() => { vi.useRealTimers(); });
const actions = () => ({ save: vi.fn(async () => true), select: vi.fn(async (_path: string) => {}), error: vi.fn() });

it('saves a shared note window before closing without polling external paths', async () => {
  const { save, error } = actions();
  await setupWorkspace('document-note-a', { save, error });
  expect(invoke).not.toHaveBeenCalledWith('workspace_paths');
  expect(handlers.has('workspace-select-request')).toBe(false);
  await handlers.get('workspace-before-close')!({ payload: null });
  expect(save).toHaveBeenCalledOnce();
  expect(invoke).toHaveBeenLastCalledWith('workspace_finish_close');
});

it('saves before switching within a directory window and consumes requests queued during startup', async () => {
  const current = actions(), order: string[] = [];
  current.save.mockImplementation(async () => { order.push('save'); return true; });
  current.select.mockImplementation(async path => { order.push(path); });
  paths.push('/Downloads/b.md'); await setupWorkspace('document-a', current);
  expect(order).toEqual(['save', '/Downloads/b.md']);
  paths.push('/Downloads/c.md', '/Downloads/a.md'); await handlers.get('workspace-select-request')!({ payload: null });
  expect(order).toEqual(['save', '/Downloads/b.md', 'save', '/Downloads/c.md', 'save', '/Downloads/a.md']);
  expect(current.select).toHaveBeenCalledTimes(3); expect(document.body.inert).toBe(false);
});

it('keeps the old document on failed save or unreadable destination and releases the editing lock', async () => {
  const current = actions(); await setupWorkspace('document-a', current);
  current.save.mockResolvedValueOnce(false); paths.push('/Downloads/b.md');
  await handlers.get('workspace-select-request')!({ payload: null });
  expect(current.select).not.toHaveBeenCalled(); expect(current.error).toHaveBeenCalledWith(expect.stringContaining('尚未保存'));
  current.select.mockRejectedValueOnce(new Error('destination removed')); paths.push('/Downloads/b.md');
  await handlers.get('workspace-select-request')!({ payload: null });
  expect(current.error).toHaveBeenCalledWith('destination removed'); expect(document.body.inert).toBe(false);
});

it('waits for a directory switch before saving and closing that window', async () => {
  const current = actions(); let complete!: () => void;
  await setupWorkspace('document-a', current);
  current.select.mockImplementationOnce(() => new Promise<void>(resolve => { complete = resolve; }));
  paths.push('/Downloads/b.md'); const change = handlers.get('workspace-select-request')!({ payload: null });
  await vi.waitFor(() => expect(current.select).toHaveBeenCalledOnce());
  const close = handlers.get('workspace-before-close')!({ payload: null });
  expect(invoke).not.toHaveBeenCalledWith('workspace_finish_close');
  complete(); await change; await close;
  expect(current.save).toHaveBeenCalledTimes(2); expect(invoke).toHaveBeenCalledWith('workspace_finish_close');
});

it('finishes rapid same-directory requests in order, including reopening the previous file during a switch', async () => {
  const current = actions(); let complete!: () => void;
  await setupWorkspace('document-a', current);
  current.select.mockImplementationOnce(() => new Promise<void>(resolve => { complete = resolve; }));
  paths.push('/Downloads/b.md'); const first = handlers.get('workspace-select-request')!({ payload: null });
  await vi.waitFor(() => expect(current.select).toHaveBeenCalledOnce());
  paths.push('/Downloads/a.md'); const second = handlers.get('workspace-select-request')!({ payload: null });
  complete(); await Promise.all([first, second]);
  expect(current.select.mock.calls.map(([path]) => path)).toEqual(['/Downloads/b.md', '/Downloads/a.md']);
  expect(document.body.inert).toBe(false);
});

it('preserves the old snapshot when a switch fails and cancels delayed saves captured before a successful switch', async () => {
  const book = new Notebook({ getItem: () => null, setItem: () => {} }, '');
  vi.mocked(invoke).mockResolvedValueOnce({ library: structuredClone(disk), root: '/Downloads', revision: 1 });
  await book.attachExternal(); const before = JSON.stringify(book.data);
  vi.mocked(invoke).mockRejectedValueOnce('destination removed');
  await expect(book.activateExternal('/Downloads/b.md')).rejects.toBe('destination removed');
  expect(JSON.stringify(book.data)).toBe(before);
  const next = structuredClone(disk); next.notes.push({ ...next.notes[0], id: 'b', path: 'b.md', source: 'B' }); next.activeId = 'b';
  vi.mocked(invoke).mockResolvedValueOnce({ library: next, root: '/Downloads', revision: 2 });
  const change = book.activateExternal('/Downloads/b.md'), stale = book.save('late A input', 'external');
  await change; await expect(stale).rejects.toThrow('旧的延迟保存已取消');
  expect(book.current.id).toBe('b'); expect(book.current.source).toBe('B');
});

it('delivers open, close and quit events only to their owning window, including Tauri global-listener semantics', async () => {
  const first = actions(), second = actions();
  await setupWorkspace('document-a', first); await setupWorkspace('document-b', second);
  const emit = async (target: string, name: string, payload: unknown = null) => {
    await Promise.all(listeners.filter(listener => listener.name === name && (!listener.target || listener.target === target)).map(listener => listener.handler({ payload })));
  };
  vi.mocked(invoke).mockClear();
  await emit('document-a', 'library-command', 'workspace-open');
  expect(invoke).toHaveBeenCalledExactlyOnceWith('workspace_open', { path: undefined });
  vi.mocked(invoke).mockClear();
  await emit('document-a', 'workspace-before-close');
  expect(first.save).toHaveBeenCalledOnce(); expect(second.save).not.toHaveBeenCalled();
  expect(invoke).toHaveBeenCalledExactlyOnceWith('workspace_finish_close');
  await emit('document-b', 'workspace-before-exit', 'quit-round');
  expect(first.save).toHaveBeenCalledOnce(); expect(second.save).toHaveBeenCalledOnce();
  expect(invoke).toHaveBeenLastCalledWith('workspace_exit_reply', { id: 'quit-round', saved: true });
});

it('routes cold and later OS requests to independent windows without saving or replacing the current editor', async () => {
  const current = actions(); requests.push('/Downloads/a.md', '/Downloads/b.md');
  document.body.dataset.syncBusy = 'true';
  await setupWorkspace('main', current);
  expect(invoke).toHaveBeenCalledWith('workspace_open', { path: '/Downloads/a.md' });
  expect(invoke).toHaveBeenCalledWith('workspace_open', { path: '/Downloads/b.md' });
  expect(current.save).not.toHaveBeenCalled(); expect(document.querySelector<HTMLButtonElement>('#workspace-back')!.hidden).toBe(true);
  requests.push('/other/c.md'); await handlers.get('workspace-request')!({ payload: null });
  expect(invoke).toHaveBeenCalledWith('workspace_open', { path: '/other/c.md' });
  expect(document.body.dataset.syncBusy).toBe('true');
});

it('keeps processing file requests after a rejected open and supports Cmd+O from document windows', async () => {
  const current = actions(); await setupWorkspace('document-a', current);
  expect(invoke).not.toHaveBeenCalledWith('workspace_requests');
  vi.mocked(invoke).mockRejectedValueOnce('cannot read');
  await handlers.get('library-command')!({ payload: 'workspace-open' });
  expect(current.error).toHaveBeenCalledWith('cannot read');
  await handlers.get('library-command')!({ payload: 'workspace-open' });
  expect(invoke).toHaveBeenLastCalledWith('workspace_open', { path: undefined });
  expect(current.save).not.toHaveBeenCalled();
});

it('saves a document before closing only that window and blocks close on unfinished input', async () => {
  const current = actions(); await setupWorkspace('document-a', current); current.save.mockResolvedValueOnce(false);
  await handlers.get('workspace-before-close')!({ payload: null });
  expect(invoke).not.toHaveBeenCalledWith('workspace_finish_close'); expect(document.body.inert).toBe(false);
  await handlers.get('workspace-before-close')!({ payload: null });
  expect(invoke).toHaveBeenCalledWith('workspace_finish_close'); expect(invoke).not.toHaveBeenCalledWith('library_finish_exit');
});

it('returns to the library by focusing it, preserving the document window', async () => {
  const current = actions(); await setupWorkspace('document-a', current);
  await handlers.get('library-command')!({ payload: 'workspace-close' });
  expect(current.save).toHaveBeenCalledOnce(); expect(invoke).toHaveBeenCalledWith('workspace_show_library');
  expect(invoke).not.toHaveBeenCalledWith('workspace_finish_close');
});

it('acknowledges app quit only after pending filesystem work and local saves, then unfreezes on cancellation', async () => {
  vi.useFakeTimers(); const current = actions(); await setupWorkspace('document-a', current);
  document.body.dataset.libraryBusy = 'true';
  const exit = handlers.get('workspace-before-exit')!({ payload: 'round-1' });
  expect(current.save).not.toHaveBeenCalled(); expect(document.body.inert).toBe(true);
  document.body.dataset.libraryBusy = 'false'; await vi.advanceTimersByTimeAsync(50); await exit;
  expect(invoke).toHaveBeenCalledWith('workspace_exit_reply', { id: 'round-1', saved: true });
  await handlers.get('workspace-before-exit')!({ payload: 'round-1' }); expect(current.save).toHaveBeenCalledOnce();
  await handlers.get('workspace-exit-cancelled')!({ payload: null }); expect(document.body.inert).toBe(false);
  current.save.mockResolvedValueOnce(false);
  await handlers.get('workspace-before-exit')!({ payload: 'round-2' });
  expect(invoke).toHaveBeenCalledWith('workspace_exit_reply', { id: 'round-2', saved: false });
});

it('recovers a quit request received before document listeners were ready', async () => {
  vi.mocked(invoke).mockResolvedValueOnce('startup-round'); const current = actions();
  await setupWorkspace('document-a', current);
  expect(invoke).toHaveBeenCalledWith('workspace_exit_reply', { id: 'startup-round', saved: true });
});

it('attaches only its own native snapshot and never enrols the document in S3 or localStorage', async () => {
  const storage = { getItem: () => null, setItem: vi.fn() }, book = new Notebook(storage, '');
  vi.mocked(invoke).mockResolvedValueOnce({ library: structuredClone(disk), root: '/Downloads', revision: 1 });
  await book.attachExternal(); expect(invoke).toHaveBeenLastCalledWith('workspace_snapshot');
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === 'workspace_select') return null;
    if (command === 'workspace_reconcile') return { snapshot: null, backups: [] };
    return { library: (args as any).next, root: '/Downloads', revision: 2 };
  });
  expect(await book.select('external')).toBe(false); expect(book.current.id).toBe('external');
  await book.save('edited source'); expect(invoke).toHaveBeenLastCalledWith('workspace_commit', expect.objectContaining({ next: expect.objectContaining({ notes: [expect.objectContaining({ source: 'edited source' })] }) }));
  await expect(book.readSyncAssets()).rejects.toThrow('不在 S3');
  await expect(book.commitSync(book.data, {} as any, book.data)).rejects.toThrow('不在 S3');
  await book.reload(); expect(invoke).toHaveBeenLastCalledWith('workspace_reconcile', expect.objectContaining({ root: '/Downloads' }));
  expect(storage.setItem).not.toHaveBeenCalled();
});
