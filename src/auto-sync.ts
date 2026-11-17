import type { SyncClient, SyncStatus } from './sync-session';
import { syncMode } from './sync-store';
import type { PreferenceStore } from './preferences';

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
      if (schedule?.value.autoSyncS3 === false ||
        !settings.endpoint.trim() || !settings.bucket.trim() || !settings.prefix.trim() || !settings.accessKeyId.trim() || !settings.secretAccessKey) return;
      await client.run(settings);
    })().finally(() => { pending = undefined; lastAttempt = Date.now(); });
    return pending;
  }
  const attempt = () => { if (!blocked()) void sync().catch(reason => error(reason instanceof Error ? reason.message : typeof reason === 'string' ? reason : '自动同步失败')); };
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
