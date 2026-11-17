// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest';
import { startAutoSync } from '../src/auto-sync';
import type { SyncStatus, SyncClient } from '../src/sync-session';
import type { S3Settings } from '../src/sync-store';
import type { Preferences, PreferenceStore } from '../src/preferences';

const settings = { endpoint: 'https://unit.invalid', bucket: 'test', prefix: 'notes', region: 'us-east-1', accessKeyId: 'test', secretAccessKey: 'test' };
let dispose: (() => void) | undefined;
afterEach(() => { dispose?.(); dispose = undefined; vi.useRealTimers(); });
function fixture(blocked = () => false, onBackground = false, initial: S3Settings = settings, schedule?: Pick<PreferenceStore, 'subscribe' | 'value'>) {
  vi.useFakeTimers();
  const listeners = new Set<(s: SyncStatus) => void>();
  let status: SyncStatus = { busy: false, error: false, message: '' };
  const client: SyncClient = { load: vi.fn(async () => initial), save: vi.fn(), flush: vi.fn(), run: vi.fn(async () => {}),
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
