import { invoke } from '@tauri-apps/api/core';
import type { Notebook } from './notebook';
import { syncNotebook } from './sync';
import { normalizeSettings, type S3Settings } from './sync-store';
import { setRequestHandler } from './vendor/mdsync/core';

export type SyncStatus = { busy: boolean; message: string; error: boolean };
export interface SyncClient {
  load(): Promise<S3Settings>;
  save(settings: S3Settings): Promise<void>;
  flush(): Promise<void>;
  run(settings: S3Settings): Promise<void>;
  subscribe(listener: (status: SyncStatus) => void): () => void;
}
const profileKey = 'miaoyan-editor.s3-profile.v1';
const fields = ['endpoint', 'bucket', 'region', 'prefix', 'accessKeyId', 'secretAccessKey'] as const;
function profile(value: S3Settings): S3Settings {
  if (!value || fields.some(key => typeof value[key] !== 'string' || value[key].length > 4096 || /[\x00-\x1f\x7f-\x9f]/.test(value[key]))) throw new Error('连接字段无效');
  return Object.fromEntries(fields.map(key => [key, value[key]])) as S3Settings;
}
export function createSyncSession(book: Notebook, refresh: () => void, prepare: () => boolean | Promise<boolean>): SyncClient {
  let config: S3Settings = { endpoint: '', bucket: '', region: 'us-east-1', prefix: 'miaoyan/v1', accessKeyId: '', secretAccessKey: '' };
  try {
    const saved = JSON.parse(localStorage.getItem(profileKey) ?? '{}');
    for (const key of ['endpoint', 'bucket', 'region', 'prefix'] as const) if (typeof saved[key] === 'string') config[key] = saved[key];
  } catch { /* Optional connection profile does not affect notes. */ }
  let loaded = false, hasSaved = false;
  let writes = Promise.resolve();
  async function load() {
    if (!loaded) {
      if (!('__TAURI_INTERNALS__' in window)) throw new Error('请在应用中保存 S3 连接配置');
      const saved = await invoke<S3Settings | null>('sync_load_profile');
      if (saved) { config = profile(saved); hasSaved = true; }
      loaded = true;
    }
  }
  async function save(settings: S3Settings) {
    const next = profile(settings);
    writes = writes.catch(() => {}).then(async () => {
      await load();
      if (hasSaved && JSON.stringify(config) === JSON.stringify(next)) return;
      await invoke('sync_save_profile', { profile: next });
      config = next; hasSaved = true;
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
    async run(settings) {
      if (status.busy) throw new Error('同步正在进行，请稍候');
      publish({ busy: true, error: false });
      try {
        await save(settings);
        const next = normalizeSettings(settings);
        if (!('__TAURI_INTERNALS__' in window)) throw new Error('请在应用中使用 S3 同步');
        if (!await prepare()) throw new Error('当前输入尚未保存，请完成输入后再次同步');
        publish({ message: '正在连接 S3…' });
        await invoke('sync_set_target', { endpoint: next.endpoint, bucket: next.bucket, prefix: next.prefix });
        setRequestHandler(async request => {
          const reply = await invoke<{ status: number; headers: Record<string, string>; body: number[] }>('sync_request', {
            request: { url: request.url, method: request.method, headers: request.headers ?? {}, body: request.body ? Array.from(new Uint8Array(request.body)) : null },
          });
          const bytes = new Uint8Array(reply.body);
          return { status: reply.status, headers: reply.headers, arrayBuffer: bytes.buffer, text: new TextDecoder().decode(bytes), json: {} };
        });
        const result = await syncNotebook(book, next, message => publish({ message }), prepare);
        refresh(); publish({ message: `同步完成：${result.notes} 篇笔记、${result.folders} 个文件夹、${result.assets ?? 0} 个附件${result.conflicts ? '，冲突已保留双方' : ''}` });
      } catch (error) {
        let message = error instanceof Error ? error.message : typeof error === 'string' ? error : '同步失败，请检查连接后重试';
        for (const secret of [settings.accessKeyId, settings.secretAccessKey]) if (secret) message = message.replaceAll(secret, '[已隐藏]');
        publish({ error: true, message });
        throw new Error(message);
      } finally { publish({ busy: false }); }
    },
  };
}
