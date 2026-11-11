// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { Notebook, type Library } from '../src/notebook';
import { setupWorkspace } from '../src/workspace';

const handlers = vi.hoisted(() => new Map<string, (event: { payload: any }) => unknown>());
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async (name, handler) => { handlers.set(name, handler); return () => handlers.delete(name); }) }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
const disk: Library = { version: 1, activeId: 'external', folders: [{ id: 'default', name: 'Downloads', path: '' }], notes: [{ id: 'external', folderId: 'default', path: 'file.md', filename: 'file.md', title: 'file', source: 'original', createdAt: 1, updatedAt: 1 }] };
let requests: string[], opened: string[], saved: string[], diskLibrary: Library;
beforeEach(() => {
  vi.clearAllMocks(); handlers.clear(); requests = []; opened = []; saved = []; diskLibrary = structuredClone(disk);
  document.body.innerHTML = '<div id="app-shell"><button id="workspace-back" hidden></button></div>';
  document.body.inert = false; delete document.body.dataset.libraryBusy; delete document.body.dataset.syncBusy;
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === 'workspace_requests') return requests.splice(0);
    if (command === 'workspace_close') return;
    if (command === 'workspace_open') opened.push((args as { path: string }).path);
    if (command === 'workspace_commit') { diskLibrary = structuredClone((args as { next: Library }).next); saved.push(command); }
    if (command === 'workspace_reconcile') return { snapshot: null, backups: [] };
    return { library: structuredClone(diskLibrary), root: '/Downloads', revision: 1 };
  });
});
afterEach(() => { vi.useRealTimers(); });
async function fixture() {
  const storage = { getItem: () => null, setItem: vi.fn() }, library = new Notebook(storage, 'library');
  let current = library;
  const actions = { current: () => current, save: vi.fn(async () => true), activate: vi.fn(async (book: Notebook) => { current = book; }), error: vi.fn() };
  const workspace = await setupWorkspace(library, actions);
  return { library, storage, actions, workspace, current: () => current };
}

it('handles cold and subsequent file opens after saving, and returns without changing the configured library', async () => {
  requests.push('/Downloads/file.md'); const { library, actions, current } = await fixture();
  const original = JSON.stringify(library.data);
  expect(opened).toEqual(['/Downloads/file.md']); expect(actions.save).toHaveBeenCalledOnce(); expect(current().external).toBe(true);
  await current().save('edited source'); expect(saved).toEqual(['workspace_commit']);
  expect(JSON.stringify(library.data)).toBe(original);
  requests.push('/other/second.md'); await handlers.get('workspace-request')!({ payload: null });
  expect(opened).toEqual(['/Downloads/file.md', '/other/second.md']);
  document.querySelector<HTMLButtonElement>('#workspace-back')!.click();
  await vi.waitFor(() => expect(current()).toBe(library));
  expect(invoke).toHaveBeenCalledWith('workspace_close'); expect(document.querySelector<HTMLButtonElement>('#workspace-back')!.hidden).toBe(true);
});

it('retains the current workspace on unfinished input or a cancelled file picker', async () => {
  const { library, actions, current } = await fixture(); actions.save.mockResolvedValueOnce(false);
  requests.push('/Downloads/file.md'); await handlers.get('workspace-request')!({ payload: null });
  expect(opened).toEqual([]); expect(current()).toBe(library);
  vi.mocked(invoke).mockResolvedValueOnce(null);
  await handlers.get('library-command')!({ payload: 'workspace-open' });
  await vi.waitFor(() => expect(document.body.dataset.libraryBusy).toBe('false'));
  expect(current()).toBe(library); expect(actions.activate).not.toHaveBeenCalled();
});

it('queues operating-system opens during sync and refuses enrolling external files in S3', async () => {
  const { current, storage } = await fixture(); vi.useFakeTimers(); document.body.dataset.syncBusy = 'true';
  requests.push('/Downloads/file.md'); await handlers.get('workspace-request')!({ payload: null }); expect(opened).toEqual([]);
  document.body.dataset.syncBusy = 'false'; await vi.advanceTimersByTimeAsync(250);
  expect(current().external).toBe(true);
  await expect(current().readSyncAssets()).rejects.toThrow('不在 S3');
  await expect(current().commitSync(current().data, {} as any, JSON.stringify(current().data))).rejects.toThrow('不在 S3');
  await current().reload(); expect(invoke).toHaveBeenCalledWith('workspace_reconcile', expect.objectContaining({ root: '/Downloads' }));
  expect(storage.setItem).not.toHaveBeenCalled();
});
