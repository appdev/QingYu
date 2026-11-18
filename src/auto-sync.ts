import { accountSyncNotices, type SyncClient, type SyncStatus } from './sync-session';
import { hasS3Connection, syncMode } from './sync-store';
import type { PreferenceStore } from './preferences';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export async function connectCloudAutoSync(client: SyncClient, preferences: PreferenceStore, sync: () => Promise<void>, error: (message: string) => void) {
  let pending = Promise.resolve();
  const refresh = () => {
    pending = pending.catch(() => {}).then(async () => {
      const account = await invoke<{ profile: { id: string } } | null>('account_status');
      const id = account?.profile.id ?? '';
      if (id === preferences.value.cloudSyncAccount) return;
      if (!id) { preferences.update({ cloudSyncAccount: '', autoSyncCloudAccount: '' }); return; }
      let unsubscribe = () => {};
      await new Promise<void>(resolve => { unsubscribe = client.subscribe(status => { if (!status.busy) resolve(); }); });
      unsubscribe();
      await client.save({ ...await client.load(), mode: 'cloud' });
      preferences.update({ cloudSyncAccount: id, autoSyncCloudAccount: id });
      await sync();
    });
    void pending.catch(reason => error(reason instanceof Error ? reason.message : typeof reason === 'string' ? reason : '无法启用云同步'));
    return pending;
  };
  const stop = await listen('client-profile-changed', () => { void refresh(); });
  await refresh().catch(() => {});
  return stop;
}

export function startAutoSync(client: SyncClient, blocked: () => boolean, error: (message: string) => void, syncOnBackground = false,
  schedule?: Pick<PreferenceStore, 'subscribe' | 'value'>) {
  let status: SyncStatus = { busy: false, error: false, message: '' };
  let pending: Promise<void> | undefined;
  let lastAttempt = Date.now();
  const unsubscribe = client.subscribe(next => { status = next; });
  function joinCurrent() {
    return new Promise<void>((resolve, reject) => {
      const stop = client.subscribe(next => {
        if (!next.busy) { stop(); next.error ? reject(new Error(next.message)) : resolve(); }
      });
    });
  }
  function sync(): Promise<void> {
    if (pending) return pending;
    pending = (async () => {
      if (status.busy) {
        await joinCurrent();
        return;
      }
      const settings = await client.load();
      if (status.busy) { await joinCurrent(); return; }
      if (syncMode(settings) === 'cloud') {
        const id = schedule?.value.autoSyncCloudAccount;
        if (id) await client.run(settings, id);
        return;
      }
      if (schedule?.value.autoSyncS3 === false || !hasS3Connection(settings)) return;
      await client.run(settings);
    })().finally(() => { pending = undefined; lastAttempt = Date.now(); });
    return pending;
  }
  const attempt = () => {
    if (blocked()) return;
    void sync().catch(reason => {
      const message = reason instanceof Error ? reason.message : typeof reason === 'string' ? reason : '自动同步失败';
      // 没登录或账号未就绪时后台静默跳过，手动同步时设置页里仍会显示原因。
      if (!accountSyncNotices.has(message)) error(message);
    });
  };
  const intervalMs = () => (schedule?.value.syncInterval ?? 3) * 60 * 1000;
  let interval = intervalMs(), timer = setInterval(attempt, interval);
  const unsubscribeSchedule = schedule?.subscribe(() => {
    const next = intervalMs();
    if (next === interval) return;
    clearInterval(timer); interval = next; timer = setInterval(attempt, interval);
  });
  const visibility = () => {
    // Mobile processes can be killed without a quit event; try while entering the background.
    if (document.hidden ? syncOnBackground : Date.now() - lastAttempt >= interval) attempt();
  };
  document.addEventListener('visibilitychange', visibility);
  attempt();
  return { sync, stop() { clearInterval(timer); document.removeEventListener('visibilitychange', visibility); unsubscribe(); unsubscribeSchedule?.(); } };
}
