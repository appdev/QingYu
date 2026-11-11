import type { SyncClient, SyncStatus } from './sync-session';

export function startAutoSync(client: SyncClient, blocked: () => boolean, error: (message: string) => void, syncOnBackground = false) {
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
      if (!settings.endpoint.trim() || !settings.bucket.trim() || !settings.prefix.trim() || !settings.accessKeyId.trim() || !settings.secretAccessKey) return;
      await client.run(settings);
    })().finally(() => { pending = undefined; lastAttempt = Date.now(); });
    return pending;
  }
  const attempt = () => { if (!blocked()) void sync().catch(reason => error(reason instanceof Error ? reason.message : typeof reason === 'string' ? reason : '自动同步失败')); };
  const timer = setInterval(attempt, 3 * 60 * 1000);
  const visibility = () => {
    // Mobile processes can be killed without a quit event; try while entering the background.
    if (document.hidden ? syncOnBackground : Date.now() - lastAttempt >= 3 * 60 * 1000) attempt();
  };
  document.addEventListener('visibilitychange', visibility);
  attempt();
  return { sync, stop() { clearInterval(timer); document.removeEventListener('visibilitychange', visibility); unsubscribe(); } };
}
