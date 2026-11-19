import { preferences } from './preferences';
import { cloudErrors, type SyncClient, type SyncStatus } from './sync-session';
import type { S3Settings } from './sync-store';

// 网页客户端的同步。用法和 App 的同步会话相同：设置页填写 S3 或选择轻羽云存储、测试连接、立即同步。
// 区别只在执行位置：笔记在部署端的数据目录里，所以连接配置保存在部署端的配置目录，同步也由部署端运行。

type ServerStatus = {
  busy: boolean; error: boolean; message: string;
  lastSyncedAt: number | null; lastSyncedAccount: string | null; unavailable?: string;
};
type Schedule = { s3: boolean; cloudAccount: string; interval: number };

const emptyProfile: S3Settings = { endpoint: '', bucket: '', region: 'us-east-1', prefix: 'miaoyan/v1', accessKeyId: '', secretAccessKey: '' };

async function call<T>(path: string, method = 'GET', body?: unknown): Promise<T> {
  const response = await fetch(`/api/web/sync${path}`, body === undefined ? { method, cache: 'no-store' }
    : { method, cache: 'no-store', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
  if (!response.ok) {
    const reply = await response.json().catch(() => ({})) as { error?: string };
    throw new Error(reply.error || '无法连接网页客户端服务，请稍后重试');
  }
  return (response.status === 204 ? undefined : await response.json()) as T;
}

export function createServerSyncClient(): SyncClient {
  const listeners = new Set<(status: SyncStatus) => void>();
  let status: SyncStatus = { busy: false, message: '', error: false };
  let timer = 0, revision = 0;
  let stopSchedule = () => {};
  // 测试连接的结果只在本页显示；部署端的同步状态没有变化前，轮询不覆盖它
  let held: { server: string; message: string; error: boolean } | undefined;
  let lastServer = '', testing = false;

  function emit(change: Partial<SyncStatus>) {
    status = { ...status, ...change };
    for (const listener of listeners) listener(status);
  }
  function apply(server: ServerStatus) {
    if (status.busy && !server.busy && !server.error && !testing) dispatchEvent(new Event('web-sync-finished'));
    lastServer = JSON.stringify(server);
    if (held && held.server !== lastServer) held = undefined;
    const message = held?.message ?? (server.error && Object.hasOwn(cloudErrors, server.message) ? cloudErrors[server.message] : server.message);
    emit({ busy: server.busy || testing, error: held?.error ?? server.error, message, lastSyncedAt: server.lastSyncedAt ?? undefined,
      lastSyncedAccount: server.lastSyncedAccount ?? undefined, unavailable: server.unavailable });
    return server;
  }
  function hold(message: string, error: boolean) {
    held = { server: lastServer, message, error };
    emit({ message, error });
  }
  async function poll() {
    clearTimeout(timer);
    const current = ++revision;
    let server: ServerStatus | undefined;
    try { server = await call<ServerStatus>(''); }
    catch (reason) { if (current === revision) emit({ unavailable: reason instanceof Error ? reason.message : '无法读取同步状态' }); }
    if (current !== revision) return server;
    if (server) apply(server);
    if (listeners.size) timer = window.setTimeout(() => void poll(), server?.busy ? 1000 : 3000);
    return server;
  }

  // 自动同步开关和间隔沿用 App 的偏好设置项，但网页客户端由部署端按它们定时同步，所以和部署端保持一致
  function watchSchedule() {
    let synced: string | undefined, dirty = false;
    const current = () => JSON.stringify({ s3: preferences.value.autoSyncS3, cloudAccount: preferences.value.autoSyncCloudAccount, interval: preferences.value.syncInterval });
    const push = () => {
      const next = current();
      if (next === synced) return;
      synced = next;
      void call('/schedule', 'PUT', JSON.parse(next)).catch(reason => emit({ error: true, message: reason instanceof Error ? reason.message : '无法保存自动同步设置' }));
    };
    // 部署端的设置读回来之前，本地已有改动（例如刚登录时开启云同步）就以本地为准
    const unsubscribe = preferences.subscribe(() => { if (synced === undefined) dirty = true; else push(); });
    void call<Schedule>('/schedule').then(schedule => {
      synced = JSON.stringify(schedule);
      if (dirty) { push(); return; }
      preferences.update({ autoSyncS3: schedule.s3, autoSyncCloudAccount: schedule.cloudAccount, syncInterval: schedule.interval as typeof preferences.value.syncInterval });
    }).catch(() => {});
    return unsubscribe;
  }

  async function save(settings: S3Settings) { await call('/profile', 'PUT', settings); }

  return {
    async load() { return { ...emptyProfile, ...await call<S3Settings | null>('/profile') }; },
    save,
    async flush() {},
    async test(settings) {
      if (status.busy) throw new Error('同步正在进行，请稍候');
      testing = true;
      held = { server: lastServer, message: '正在测试 S3 连接…', error: false };
      emit({ busy: true, error: false, message: held.message });
      try {
        await call('/test', 'POST', settings);
        hold('连接测试成功：读取、写入、条件写入和删除均正常', false);
      } catch (reason) {
        const message = reason instanceof Error ? reason.message : '连接测试失败';
        hold(message, true);
        throw new Error(message);
      } finally { testing = false; emit({ busy: false }); }
    },
    async run(settings, accountId) {
      if (status.busy) throw new Error('同步正在进行，请稍候');
      if (!accountId) await save({ ...settings, mode: undefined });
      held = undefined;
      apply(await call<ServerStatus>('', 'POST', { accountId }));
      // 同步在部署端后台进行，等它结束再返回，和 App 的 run 一样在失败时抛出原因
      for (;;) {
        await new Promise(resolve => setTimeout(resolve, 1000));
        const server = await poll();
        if (server && !server.busy) {
          if (server.error) throw new Error(status.message);
          return;
        }
      }
    },
    subscribe(listener) {
      listeners.add(listener);
      listener(status);
      if (listeners.size === 1) { void poll(); stopSchedule = watchSchedule(); }
      return () => {
        listeners.delete(listener);
        if (!listeners.size) { clearTimeout(timer); revision++; stopSchedule(); }
      };
    },
  };
}
