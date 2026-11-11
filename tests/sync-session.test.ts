// @vitest-environment jsdom
import { beforeEach, expect, it, vi } from 'vitest';
import { createSyncSession, type SyncStatus } from '../src/sync-session';
import { invoke } from '@tauri-apps/api/core';
import { syncNotebook } from '../src/sync';
import type { Notebook } from '../src/notebook';
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue(undefined) }));
vi.mock('../src/sync', () => ({ syncNotebook: vi.fn() }));
const config = { endpoint: 'https://unit.invalid', bucket: 'test', region: 'us-east-1', prefix: 'test', accessKeyId: 'private-id', secretAccessKey: 'private-secret' };
let persisted: typeof config | null;
beforeEach(() => {
  localStorage.clear(); vi.clearAllMocks(); persisted = null; Object.assign(window, { __TAURI_INTERNALS__: {} });
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === 'sync_load_profile') return persisted;
    if (command === 'sync_save_profile') { persisted = structuredClone((args as { profile: typeof config }).profile); return; }
  });
});
it('persists the complete profile through native app configuration and restores AK/SK in a new session', async () => {
  const session = createSyncSession({} as Notebook, vi.fn(), () => true);
  await session.save(config);
  expect(await session.load()).toMatchObject(config);
  expect(persisted).toEqual(config); expect(localStorage.length).toBe(0);
  const reopened = createSyncSession({} as Notebook, vi.fn(), () => true);
  expect(await reopened.load()).toEqual(config);
});
it('saves incomplete drafts in order, waits on exit, and keeps legacy nonsecret fields', async () => {
  localStorage.setItem('miaoyan-editor.s3-profile.v1', JSON.stringify({ ...config, accessKeyId: 'ignored', secretAccessKey: 'ignored' }));
  const session = createSyncSession({} as Notebook, vi.fn(), () => true);
  expect(await session.load()).toMatchObject({ endpoint: config.endpoint, accessKeyId: '', secretAccessKey: '' });
  const a = session.save({ ...config, accessKeyId: '', secretAccessKey: '' });
  const b = session.save({ ...config, secretAccessKey: 'new-secret' });
  await session.flush(); await Promise.all([a, b]);
  expect(persisted?.secretAccessKey).toBe('new-secret'); expect(localStorage.length).toBe(0);
});
it('does not replace stored credentials when loading or saving fails', async () => {
  persisted = config;
  const session = createSyncSession({} as Notebook, vi.fn(), () => true); await session.load();
  vi.mocked(invoke).mockRejectedValueOnce(new Error('凭据存储已锁定'));
  await expect(session.save({ ...config, secretAccessKey: '' })).rejects.toThrow('已锁定');
  await expect(session.flush()).rejects.toThrow('已锁定'); expect(persisted).toEqual(config);
  expect(await session.load()).toEqual(config);
  await session.save(config); await expect(session.flush()).resolves.toBeUndefined();
  const another = createSyncSession({} as Notebook, vi.fn(), () => true);
  vi.mocked(invoke).mockRejectedValueOnce(new Error('读取失败'));
  await expect(another.load()).rejects.toThrow('读取失败'); expect(persisted).toEqual(config);
});
it('serializes simultaneous requests before asynchronous editor flushing and recovers on failure', async () => {
  let finish!: (value: boolean) => void; const prepare = vi.fn(() => new Promise<boolean>(resolve => { finish = resolve; }));
  const session = createSyncSession({} as Notebook, vi.fn(), prepare), states: SyncStatus[] = [];
  session.subscribe(status => states.push(status));
  const first = session.run(config);
  await vi.waitFor(() => expect(prepare).toHaveBeenCalledOnce());
  await expect(session.run(config)).rejects.toThrow('正在进行'); finish(false);
  await expect(first).rejects.toThrow('当前输入尚未保存'); expect(states.at(-1)?.busy).toBe(false); expect(syncNotebook).not.toHaveBeenCalled();
});
it('passes the editor guard to commit and removes credentials from error messages', async () => {
  const prepare = vi.fn(() => true), refresh = vi.fn();
  vi.mocked(syncNotebook).mockRejectedValueOnce(new Error('private-secret private-id'));
  const session = createSyncSession({} as Notebook, refresh, prepare);
  await expect(session.run(config)).rejects.toThrow('[已隐藏] [已隐藏]');
  expect(vi.mocked(syncNotebook).mock.calls[0][3]).toBe(prepare); expect(refresh).not.toHaveBeenCalled();
});
