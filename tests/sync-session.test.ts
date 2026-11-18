// @vitest-environment jsdom
import { beforeEach, expect, it, vi } from 'vitest';
import { createSyncSession, type SyncStatus } from '../src/sync-session';
import { invoke } from '@tauri-apps/api/core';
import { syncNotebook } from '../src/sync';
import type { S3Settings } from '../src/sync-store';
import type { Notebook } from '../src/notebook';
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn().mockResolvedValue(undefined) }));
vi.mock('../src/sync', () => ({ syncNotebook: vi.fn() }));
const config = { endpoint: 'https://unit.invalid', bucket: 'test', region: 'us-east-1', prefix: 'test', accessKeyId: 'private-id', secretAccessKey: 'private-secret' };
let persisted: S3Settings | null;
beforeEach(() => {
  localStorage.clear(); vi.clearAllMocks(); persisted = null; Object.assign(window, { __TAURI_INTERNALS__: {} });
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === 'sync_load_profile') return persisted;
    if (command === 'sync_save_profile') { persisted = structuredClone((args as { profile: S3Settings }).profile); return; }
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
it('restores advanced S3 options and rejects invalid values without overwriting the profile', async () => {
  const session = createSyncSession({} as Notebook, vi.fn(), () => true);
  const settings = { ...config, timeout: 299, tlsVerify: false, forcePathStyle: false, concurrency: 16 };
  await session.save(settings);
  const reopened = createSyncSession({} as Notebook, vi.fn(), () => true);
  expect(await reopened.load()).toEqual(settings);
  await expect(reopened.save({ ...settings, timeout: 0 })).rejects.toThrow('超时');
  expect(persisted).toEqual(settings);
});
it('redacts detailed connection errors and does not sync or mark the notebook as synced', async () => {
  persisted = config;
  vi.mocked(invoke).mockImplementation(async command => {
    if (command === 'sync_load_profile') return persisted;
    if (command === 'sync_request') throw 'TLS failed private-id private-secret';
  });
  const refresh = vi.fn(), prepare = vi.fn(), states: SyncStatus[] = [];
  const session = createSyncSession({} as Notebook, refresh, prepare);
  session.subscribe(status => states.push(status));
  await expect(session.test({ ...config, timeout: 299, tlsVerify: false, forcePathStyle: false, concurrency: 16 })).rejects.toThrow('TLS failed [已隐藏] [已隐藏]');
  expect(invoke).toHaveBeenCalledWith('sync_set_target', { endpoint: config.endpoint, bucket: config.bucket, prefix: config.prefix, forcePathStyle: false, timeout: 299, tlsVerify: false });
  expect(states.at(-1)).toMatchObject({ busy: false, error: true });
  expect(states.at(-1)?.lastSyncedAt).toBeUndefined();
  expect(prepare).not.toHaveBeenCalled(); expect(refresh).not.toHaveBeenCalled(); expect(syncNotebook).not.toHaveBeenCalled();
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
  persisted = config;
  let finish!: (value: boolean) => void; const prepare = vi.fn(() => new Promise<boolean>(resolve => { finish = resolve; }));
  const session = createSyncSession({} as Notebook, vi.fn(), prepare), states: SyncStatus[] = [];
  session.subscribe(status => states.push(status));
  const first = session.run(config);
  await vi.waitFor(() => expect(prepare).toHaveBeenCalledOnce());
  await expect(session.save({ ...config, mode: 'cloud' })).rejects.toThrow('结束后可以切换');
  await expect(session.run(config)).rejects.toThrow('正在进行'); finish(false);
  await expect(first).rejects.toThrow('当前输入尚未保存'); expect(states.at(-1)?.busy).toBe(false); expect(syncNotebook).not.toHaveBeenCalled();
});
it('passes the editor guard to commit and removes credentials from error messages', async () => {
  persisted = config;
  const prepare = vi.fn(() => true), refresh = vi.fn();
  vi.mocked(syncNotebook).mockRejectedValueOnce(new Error('private-secret private-id'));
  const session = createSyncSession({} as Notebook, refresh, prepare);
  await expect(session.run(config)).rejects.toThrow('[已隐藏] [已隐藏]');
  expect(vi.mocked(syncNotebook).mock.calls[0][3]).toBe(prepare); expect(refresh).not.toHaveBeenCalled();
});
it('rejects manual sync in cloud mode, preserves the S3 profile, and allows it after reselecting S3', async () => {
  const session = createSyncSession({} as Notebook, vi.fn(), () => true);
  await expect(session.run(config)).rejects.toThrow('切换至 S3 同步');
  await session.save({ ...config, mode: 'cloud' });
  const reopened = createSyncSession({} as Notebook, vi.fn(), () => true);
  await expect(reopened.run({ ...config, mode: 's3' })).rejects.toThrow('切换至 S3 同步');
  expect(syncNotebook).not.toHaveBeenCalled();
  expect(invoke).not.toHaveBeenCalledWith('sync_set_target', expect.anything());
  expect(await reopened.load()).toEqual({ ...config, mode: 'cloud' });
  await reopened.save({ ...config, mode: 's3' });
  vi.mocked(syncNotebook).mockResolvedValue({ notes: 0, folders: 0, assets: 0, conflicts: 0 });
  await reopened.run(config);
  expect(syncNotebook).toHaveBeenCalledOnce();
});

it('uses cloud credentials for the scoped target and never saves them as a custom S3 profile', async () => {
  persisted = { ...config, mode: 'cloud' };
  const grant = { ...config, prefix: 'repo/qingyu/v1', accessKeyId: 'cloud-id', secretAccessKey: 'cloud-secret', forcePathStyle: false, concurrentReqs: 4 };
  vi.mocked(invoke).mockImplementation(async command => {
    if (command === 'sync_load_profile') return persisted;
    if (command === 'account_sync_config') return grant;
  });
  vi.mocked(syncNotebook).mockImplementation(async (_book, connection, _progress, beforeCommit) => {
    expect(connection).toMatchObject({ prefix: 'repo/qingyu/v1', forcePathStyle: false, concurrency: 4 });
    expect(await beforeCommit?.()).toBe(true);
    return { notes: 1, folders: 0, assets: 0, conflicts: 0 };
  });
  const session = createSyncSession({} as Notebook, vi.fn(), () => true);
  await session.run(config, 'account-id');
  expect(invoke).toHaveBeenCalledWith('account_sync_config', { accountId: 'account-id' });
  expect(invoke).toHaveBeenCalledWith('account_check_sync', { accountId: 'account-id' });
  expect(invoke).toHaveBeenCalledWith('sync_set_target', { endpoint: config.endpoint, bucket: config.bucket, prefix: 'repo/qingyu/v1', forcePathStyle: false, timeout: 45, tlsVerify: true });
  expect(invoke).not.toHaveBeenCalledWith('sync_save_profile', expect.anything());
  expect(await session.load()).toEqual(persisted);
});
