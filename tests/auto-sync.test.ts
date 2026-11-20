// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest';
import { connectCloudAutoSync, startAutoSync } from '../src/auto-sync';
import type { SyncStatus, SyncClient } from '../src/sync-session';
import type { S3Settings } from '../src/sync-store';
import type { Preferences, PreferenceStore } from '../src/preferences';
import { PreferenceStore as Store } from '../src/preferences';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

vi.hoisted(() => { vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {} })); });
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));

const settings = { endpoint: 'https://unit.invalid', bucket: 'test', prefix: 'notes', region: 'us-east-1', accessKeyId: 'test', secretAccessKey: 'test' };
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; vi.useRealTimers(); });
function fixture(blocked = () => false, onBackground = false, initial: S3Settings = settings, schedule?: Pick<PreferenceStore, 'subscribe' | 'value'>) {
  vi.useFakeTimers();
  const listeners = new Set<(s: SyncStatus) => void>();
  let status: SyncStatus = { busy: false, error: false, message: '' };
  const client: SyncClient = { load: vi.fn(async () => initial), save: vi.fn(), test: vi.fn(), flush: vi.fn(), run: vi.fn(async () => {}),
    subscribe(fn) { listeners.add(fn); fn(status); return () => { listeners.delete(fn); }; } };
  const error = vi.fn(), auto = startAutoSync(client, blocked, error, onBackground, schedule);
  dispose = auto.stop;
  return { auto, client, error, publish(next: SyncStatus) { status = next; for (const fn of listeners) fn(next); } };
}
it('syncs on startup, every three minutes and explicit exit; skips incomplete profiles', async () => {
  const { auto, client } = fixture();
  await vi.advanceTimersByTimeAsync(0); expect(client.run).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(179999); expect(client.run).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(1); expect(client.run).toHaveBeenCalledTimes(2);
  await auto.sync(); expect(client.run).toHaveBeenCalledTimes(3);
  vi.mocked(client.load).mockResolvedValue({ ...settings, secretAccessKey: '' });
  await auto.sync(); expect(client.run).toHaveBeenCalledTimes(3);
});
it('joins a running manual sync, propagates its failure on exit and can retry', async () => {
  const { auto, client, publish } = fixture(); await vi.advanceTimersByTimeAsync(0);
  publish({ busy: true, error: false, message: '' });
  const a = auto.sync(), b = auto.sync(); expect(a).toBe(b);
  const failure = expect(a).rejects.toThrow('连接失败');
  publish({ busy: false, error: true, message: '连接失败' }); await failure;
  await auto.sync(); expect(client.run).toHaveBeenCalledTimes(2);
});
it('pauses background work until the sync key is unlocked', async () => {
  const { auto, client, publish, error } = fixture();
  await vi.advanceTimersByTimeAsync(0);
  publish({ busy: false, error: false, message: '需要输入同步密码', passwordRequired: {} });
  auto.changed();
  await vi.advanceTimersByTimeAsync(180000);
  await auto.sync();
  expect(client.run).toHaveBeenCalledOnce();
  expect(error).not.toHaveBeenCalled();
  publish({ busy: false, error: false, message: '同步密钥已解锁' });
  await vi.advanceTimersByTimeAsync(180000);
  expect(client.run).toHaveBeenCalledTimes(2);
});
it('coalesces saved edits and queues one follow-up while another sync is running', async () => {
  const { auto, client, publish } = fixture(); await vi.advanceTimersByTimeAsync(0);
  auto.changed();
  await vi.advanceTimersByTimeAsync(300);
  auto.changed();
  await vi.advanceTimersByTimeAsync(599);
  expect(client.run).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(1);
  expect(client.run).toHaveBeenCalledTimes(2);
  publish({ busy: true, error: false, message: '' });
  auto.changed(); auto.changed();
  await vi.advanceTimersByTimeAsync(3000);
  expect(client.run).toHaveBeenCalledTimes(2);
  publish({ busy: false, error: false, message: '' });
  await vi.advanceTimersByTimeAsync(600);
  expect(client.run).toHaveBeenCalledTimes(3);
});
it('uses the selected interval and cloud account for launch, periodic and exit sync', async () => {
  let change!: (value: Preferences) => void;
  const value = { syncInterval: 3, autoSyncCloudAccount: 'bound-account', autoSyncS3: true } as Preferences;
  const schedule = { value, subscribe(callback: typeof change) { change = callback; return () => {}; } };
  const profile = { ...settings, mode: 'cloud' } as const;
  const { auto, client } = fixture(() => false, false, profile, schedule);
  await vi.advanceTimersByTimeAsync(0);
  expect(client.run).toHaveBeenCalledOnce();
  expect(client.run).toHaveBeenLastCalledWith(profile, 'bound-account');
  value.syncInterval = 1; change(value);
  await vi.advanceTimersByTimeAsync(59999);
  expect(client.run).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(1);
  expect(client.run).toHaveBeenCalledTimes(2);
  await auto.sync();
  expect(client.run).toHaveBeenCalledTimes(3);
  value.autoSyncCloudAccount = '';
  await auto.sync();
  expect(client.run).toHaveBeenCalledTimes(3);
  vi.mocked(client.load).mockResolvedValue(settings);
  value.autoSyncS3 = false;
  await auto.sync();
  expect(client.run).toHaveBeenCalledTimes(3);
});
it('skips scheduled and exit sync in cloud mode even with valid S3 details', async () => {
  const { auto, client } = fixture(() => false, false, { ...settings, mode: 'cloud' });
  await vi.advanceTimersByTimeAsync(180000);
  await auto.sync();
  expect(client.run).not.toHaveBeenCalled();
  vi.mocked(client.load).mockResolvedValue({ ...settings, mode: 's3' });
  await auto.sync();
  expect(client.run).toHaveBeenCalledOnce();
});
it('defers locked operations, retries next interval and attempts mobile background sync', async () => {
  let busy = true;
  const { client, error } = fixture(() => busy, true); await vi.advanceTimersByTimeAsync(0);
  expect(client.run).not.toHaveBeenCalled(); busy = false;
  await vi.advanceTimersByTimeAsync(180000); expect(client.run).toHaveBeenCalledOnce();
  vi.mocked(client.run).mockRejectedValueOnce(new Error('离线'));
  Object.defineProperty(document, 'hidden', { value: true, configurable: true });
  document.dispatchEvent(new Event('visibilitychange')); await vi.advanceTimersByTimeAsync(0);
  expect(error).toHaveBeenCalledWith('离线');
});
it('joins a manual sync that starts while the native profile is loading', async () => {
  const { auto, client, publish } = fixture(); await vi.advanceTimersByTimeAsync(0);
  let release!: (value: typeof settings) => void;
  vi.mocked(client.load).mockImplementationOnce(() => new Promise(resolve => { release = resolve; }));
  const pending = auto.sync(); publish({ busy: true, error: false, message: '' });
  release(settings); await vi.advanceTimersByTimeAsync(0);
  expect(client.run).toHaveBeenCalledTimes(1);
  publish({ busy: false, error: false, message: '同步完成' }); await pending;
});

it('enables cloud sync and runs once on sign-in, preserves manual off on restart, and disables it on sign-out', async () => {
  const saved = new Map<string, string>();
  const storage = { getItem: (key: string) => saved.get(key) ?? null, setItem: (key: string, value: string) => { saved.set(key, value); } };
  let preferences = new Store(storage);
  const client: SyncClient = { load: vi.fn().mockResolvedValue(settings), save: vi.fn().mockResolvedValue(undefined), test: vi.fn(), flush: vi.fn(), run: vi.fn(), subscribe: fn => { fn({ busy: false, error: false, message: '' }); return () => {}; } };
  const sync = vi.fn().mockResolvedValue(undefined), error = vi.fn();
  let changed!: () => void;
  vi.mocked(listen).mockImplementation(async (_event, handler) => { changed = () => handler({} as never); return () => {}; });
  vi.mocked(invoke).mockResolvedValue(null);
  await connectCloudAutoSync(client, preferences, sync, error);
  expect(sync).not.toHaveBeenCalled();
  vi.mocked(invoke).mockResolvedValue({ profile: { id: 'account-a' } });
  changed();
  await vi.waitFor(() => expect(sync).toHaveBeenCalledOnce());
  expect(client.save).toHaveBeenLastCalledWith({ ...settings, mode: 'cloud' });
  expect(preferences.value).toMatchObject({ autoSyncCloudAccount: 'account-a', cloudSyncAccount: 'account-a' });
  changed(); await Promise.resolve(); await Promise.resolve();
  expect(sync).toHaveBeenCalledOnce();
  preferences.update({ autoSyncCloudAccount: '' }); preferences = new Store(storage);
  await connectCloudAutoSync(client, preferences, sync, error);
  expect(sync).toHaveBeenCalledOnce(); expect(preferences.value.autoSyncCloudAccount).toBe('');
  preferences.update({ autoSyncCloudAccount: 'account-a' });
  vi.mocked(invoke).mockResolvedValue(null); changed();
  await vi.waitFor(() => expect(preferences.value.autoSyncCloudAccount).toBe(''));
  expect(preferences.value.cloudSyncAccount).toBe('');
  vi.mocked(invoke).mockResolvedValue({ profile: { id: 'account-b' } }); changed();
  await vi.waitFor(() => expect(sync).toHaveBeenCalledTimes(2));
  expect(preferences.value.autoSyncCloudAccount).toBe('account-b'); expect(error).not.toHaveBeenCalled();
});
it('waits for an ongoing sync before enabling cloud sync after login', async () => {
  const preferences = new Store({ getItem: () => null, setItem: vi.fn() });
  const { client, publish } = fixture(() => true);
  publish({ busy: true, error: false, message: '' });
  vi.mocked(invoke).mockResolvedValue({ profile: { id: 'account-a' } });
  const sync = vi.fn().mockResolvedValue(undefined);
  const pending = connectCloudAutoSync(client, preferences, sync, vi.fn());
  await vi.advanceTimersByTimeAsync(0);
  expect(client.save).not.toHaveBeenCalled();
  publish({ busy: false, error: false, message: '' }); await pending;
  expect(client.save).toHaveBeenCalledOnce(); expect(sync).toHaveBeenCalledOnce();
});
it('stays quiet when the account is signed out or not ready, but reports other failures', async () => {
  const { client, error } = fixture();
  vi.mocked(client.run).mockRejectedValueOnce(new Error('请重新登录轻羽后同步'));
  await vi.advanceTimersByTimeAsync(0);
  expect(client.run).toHaveBeenCalledTimes(1);
  expect(error).not.toHaveBeenCalled();
  vi.mocked(client.run).mockRejectedValueOnce(new Error('轻羽套餐未开通或已到期'));
  await vi.advanceTimersByTimeAsync(180000);
  expect(error).not.toHaveBeenCalled();
  vi.mocked(client.run).mockRejectedValueOnce(new Error('无法连接账号服务，请检查网络'));
  await vi.advanceTimersByTimeAsync(180000);
  expect(error).toHaveBeenCalledExactlyOnceWith('无法连接账号服务，请检查网络');
});
it('syncs once after a local change without its own timer when the deployment schedules syncing', async () => {
  vi.useFakeTimers();
  const client: SyncClient = { load: vi.fn(async () => settings), save: vi.fn(), test: vi.fn(), flush: vi.fn(), run: vi.fn(async () => {}),
    subscribe(fn) { fn({ busy: false, error: false, message: '' }); return () => {}; } };
  const auto = startAutoSync(client, () => false, vi.fn(), false, undefined, false);
  dispose = auto.stop;
  await vi.advanceTimersByTimeAsync(10 * 60 * 1000);
  expect(client.run).not.toHaveBeenCalled();
  auto.changed(); auto.changed();
  await vi.advanceTimersByTimeAsync(600);
  expect(client.run).toHaveBeenCalledTimes(1);
});
