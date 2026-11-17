import { invoke } from '@tauri-apps/api/core';
import type { Notebook } from './notebook';
import { syncNotebook } from './sync';
import { normalizeSettings, syncMode, type S3Connection, type S3Settings } from './sync-store';
import { setRequestHandler } from './vendor/mdsync/core';

export type SyncStatus = { busy: boolean; message: string; error: boolean; lastSyncedAt?: number; lastSyncedAccount?: string };
export interface SyncClient {
  load(): Promise<S3Settings>;
  save(settings: S3Settings): Promise<void>;
  flush(): Promise<void>;
  run(settings: S3Settings, accountId?: string): Promise<void>;
  subscribe(listener: (status: SyncStatus) => void): () => void;
}
const profileKey = 'miaoyan-editor.s3-profile.v1';
const fields = ['endpoint', 'bucket', 'region', 'prefix', 'accessKeyId', 'secretAccessKey'] as const;
const cloudErrors: Record<string, string> = {
  signed_out: '请重新登录轻羽后同步', account_changed: '登录账号已切换，请重新设置云同步',
  subscription_required: '轻羽套餐未开通或已到期', email_unverified: '请先在轻羽门户验证邮箱',
  storage_credentials_expiring: '存储凭据即将到期，请重试', network: '无法连接账号服务，请检查网络',
  response_lost: '未收到账号服务的响应，请重试', temporarily_unavailable: '轻羽存储暂不可用，请重试',
  invalid_response: '轻羽返回的存储配置无效，请重试', rate_limited: '请求过于频繁，请稍后重试',
};
function profile(value: S3Settings): S3Settings {
  if (!value || fields.some(key => typeof value[key] !== 'string' || value[key].length > 4096 || /[\x00-\x1f\x7f-\x9f]/.test(value[key]))) throw new Error('连接字段无效');
  if (value.mode !== undefined && value.mode !== 'cloud' && value.mode !== 's3') throw new Error('存储方式无效');
  const config = Object.fromEntries(fields.map(key => [key, value[key]])) as S3Settings;
  if (value.mode) config.mode = value.mode;
  return config;
}
export function createSyncSession(book: Notebook, refresh: () => void, prepare: () => boolean | Promise<boolean>): SyncClient {
  let config: S3Settings = { endpoint: '', bucket: '', region: 'us-east-1', prefix: 'miaoyan/v1', accessKeyId: '', secretAccessKey: '' };
  try {
    const saved = JSON.parse(localStorage.getItem(profileKey) ?? '{}');
    for (const key of ['endpoint', 'bucket', 'region', 'prefix'] as const) if (typeof saved[key] === 'string') config[key] = saved[key];
  } catch { /* Optional connection profile does not affect notes. */ }
  let loaded = false, hasSaved = false;
  let reading: Promise<void> | undefined;
  let writes = Promise.resolve();
  async function load() {
    if (loaded) return;
    if (!('__TAURI_INTERNALS__' in window)) throw new Error('请在应用中保存 S3 连接配置');
    return reading ??= invoke<S3Settings | null>('sync_load_profile').then(saved => {
      if (saved) { config = profile(saved); hasSaved = true; }
      loaded = true;
    }).finally(() => { reading = undefined; });
  }
  async function save(settings: S3Settings) {
    const next = profile(settings);
    if (next.mode && status.busy) throw new Error('同步正在进行，结束后可以切换存储方式');
    writes = writes.catch(() => {}).then(async () => {
      await load();
      if (!next.mode && config.mode) next.mode = config.mode;
      if (hasSaved && JSON.stringify(config) === JSON.stringify(next)) return;
      const switched = syncMode(config) !== syncMode(next);
      await invoke('sync_save_profile', { profile: next });
      config = next; hasSaved = true;
      if (switched) publish({ message: '', error: false, lastSyncedAt: undefined, lastSyncedAccount: undefined });
      // Native storage atomically saves the complete profile in the app's private config directory.
      try { localStorage.removeItem(profileKey); } catch { /* Legacy fields contain no credentials. */ }
    });
    return writes;
  }
  let status: SyncStatus = { busy: false, message: '', error: false };
  const listeners = new Set<(status: SyncStatus) => void>();
  function publish(change: Partial<SyncStatus>) { status = { ...status, ...change }; for (const listener of listeners) listener(status); }
  return {
    load: async () => { await writes.catch(() => {}); await load(); return { ...config }; },
    save,
    flush: () => writes,
    subscribe(listener) { listeners.add(listener); listener(status); return () => { listeners.delete(listener); }; },
    async run(settings, accountId) {
      if (status.busy) throw new Error('同步正在进行，请稍候');
      publish({ busy: true, error: false });
      let connection: S3Connection = settings;
      try {
        await writes.catch(() => {});
        await load();
        if (syncMode(config) !== (accountId ? 'cloud' : 's3')) throw new Error(accountId ? '请先切换至轻羽云存储' : '请先切换至 S3 同步');
        if (!('__TAURI_INTERNALS__' in window)) throw new Error('请在应用中使用 S3 同步');
        if (!await prepare()) throw new Error('当前输入尚未保存，请完成输入后再次同步');
        if (accountId) {
          publish({ message: '正在连接轻羽云存储…' });
          const { concurrentReqs, ...grant } = await invoke<S3Connection & { concurrentReqs: number }>('account_sync_config', { accountId });
          connection = { ...grant, concurrency: Math.max(1, Math.min(concurrentReqs || 2, 8)) };
        } else {
          connection = profile(settings);
          await save({ ...connection, mode: undefined });
        }
        const next = normalizeSettings(connection);
        publish({ message: '正在连接 S3…' });
        await invoke('sync_set_target', { endpoint: next.endpoint, bucket: next.bucket, prefix: next.prefix, forcePathStyle: next.forcePathStyle });
        setRequestHandler(async request => {
          const reply = await invoke<{ status: number; headers: Record<string, string>; body: number[] }>('sync_request', {
            request: { url: request.url, method: request.method, headers: request.headers ?? {}, body: request.body ? Array.from(new Uint8Array(request.body)) : null },
          });
          const bytes = new Uint8Array(reply.body);
          return { status: reply.status, headers: reply.headers, arrayBuffer: bytes.buffer, text: new TextDecoder().decode(bytes), json: {} };
        });
        const beforeCommit = accountId ? async () => {
          await invoke('account_check_sync', { accountId });
          return prepare();
        } : prepare;
        const result = await syncNotebook(book, next, message => publish({ message }), beforeCommit);
        refresh(); publish({ lastSyncedAt: Date.now(), lastSyncedAccount: accountId, message: `同步完成：${result.notes} 篇笔记、${result.folders} 个文件夹、${result.assets ?? 0} 个附件${result.conflicts ? '，冲突已保留双方' : ''}` });
      } catch (error) {
        let message = error instanceof Error ? error.message : typeof error === 'string' ? error : '同步失败，请检查连接后重试';
        if (accountId && Object.hasOwn(cloudErrors, message)) message = cloudErrors[message];
        for (const secret of [connection.accessKeyId, connection.secretAccessKey]) if (secret) message = message.replaceAll(secret, '[已隐藏]');
        publish({ error: true, message });
        throw new Error(message);
      } finally { publish({ busy: false }); }
    },
  };
}
