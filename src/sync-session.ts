import { invoke } from '@tauri-apps/api/core';
import { SyncSnapshotChanged, type Notebook } from './notebook';
import { syncNotebook } from './sync';
import { createStore, defaultAssetDownload, normalizeSettings, s3Defaults, syncMode, testS3Connection, validateS3Options, type S3Connection, type S3Settings } from './sync-store';
import { setRequestHandler, type Request, type Response } from './vendor/mdsync/core';
import { createAssetFetcher } from './asset-fetch';

const textEncoder = new TextEncoder(), textDecoder = new TextDecoder();

// 请求体和响应体都按原始字节走 IPC。地址、方法和签名头编码后放进 IPC 头；
// 响应开头 4 字节是状态和响应头 JSON 的长度，后面跟 JSON，再后面是正文。
export async function nativeRequest(request: Request): Promise<Response> {
  const meta = textEncoder.encode(JSON.stringify({ url: request.url, method: request.method, headers: request.headers ?? {}, hasBody: Boolean(request.body) }));
  let header = '';
  for (let offset = 0; offset < meta.length; offset += 8192) header += String.fromCharCode(...meta.subarray(offset, offset + 8192));
  const body = request.body ? new Uint8Array(request.body) : new Uint8Array(0);
  const reply = new Uint8Array(await invoke<ArrayBuffer | number[]>('sync_request', body, { headers: { 'x-qingyu-request': btoa(header) } }));
  const length = reply.length >= 4 ? new DataView(reply.buffer, reply.byteOffset, reply.byteLength).getUint32(0) : -1;
  if (length < 0 || 4 + length > reply.length) throw new Error('S3 响应格式无效');
  const { status, headers } = JSON.parse(textDecoder.decode(reply.subarray(4, 4 + length))) as Pick<Response, 'status' | 'headers'>;
  const bytes = reply.slice(4 + length);
  let text: string | undefined;
  // 大附件的正文不需要转成字符串，只有真正读取 text 时才解码。
  return { status, headers, arrayBuffer: bytes.buffer, get text() { return text ??= textDecoder.decode(bytes); }, json: {} };
}

export type SyncStatus = { busy: boolean; message: string; error: boolean; lastSyncedAt?: number; lastSyncedAccount?: string; mode?: 'cloud' | 's3' };
export interface SyncClient {
  load(): Promise<S3Settings>;
  save(settings: S3Settings): Promise<void>;
  flush(): Promise<void>;
  run(settings: S3Settings, accountId?: string): Promise<void>;
  test(settings: S3Settings): Promise<void>;
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
/** 账号状态导致的失败：未登录、登录过期、账号切换、套餐或邮箱未就绪。自动同步遇到这些时不提示。 */
export const accountSyncNotices: ReadonlySet<string> = new Set(['signed_out', 'account_changed', 'subscription_required', 'email_unverified'].map(code => cloudErrors[code]));
function profile(value: S3Settings): S3Settings {
  if (!value || fields.some(key => typeof value[key] !== 'string' || value[key].length > 4096 || /[\x00-\x1f\x7f-\x9f]/.test(value[key]))) throw new Error('连接字段无效');
  if (value.mode !== undefined && value.mode !== 'cloud' && value.mode !== 's3') throw new Error('存储方式无效');
  const config: S3Settings = { endpoint: value.endpoint, bucket: value.bucket, region: value.region, prefix: value.prefix,
    accessKeyId: value.accessKeyId, secretAccessKey: value.secretAccessKey };
  if (value.mode) config.mode = value.mode;
  validateS3Options(value);
  for (const key of ['timeout', 'tlsVerify', 'forcePathStyle', 'concurrency', 'assetDownload'] as const) if (value[key] !== undefined) Object.assign(config, { [key]: value[key] });
  return config;
}
export function createSyncSession(book: Notebook, refresh: () => void, prepare: () => boolean | Promise<boolean>, mobile = false): SyncClient & {
  fetchAsset(path: string): Promise<string | undefined>;
} {
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
      if (switched) publish({ mode: syncMode(next), message: '', error: false, lastSyncedAt: undefined, lastSyncedAccount: undefined });
      // Native storage atomically saves the complete profile in the app's private config directory.
      try { localStorage.removeItem(profileKey); } catch { /* Legacy fields contain no credentials. */ }
    });
    return writes;
  }
  let status: SyncStatus = { busy: false, message: '', error: false };
  const listeners = new Set<(status: SyncStatus) => void>();
  function publish(change: Partial<SyncStatus>) { status = { ...status, ...change }; for (const listener of listeners) listener(status); }
  async function connect(connection: S3Connection) {
    const next = normalizeSettings(connection);
    await invoke('sync_set_target', { endpoint: next.endpoint, bucket: next.bucket, prefix: next.prefix, forcePathStyle: next.forcePathStyle,
      timeout: connection.timeout ?? s3Defaults.timeout, tlsVerify: connection.tlsVerify ?? s3Defaults.tlsVerify });
    setRequestHandler(nativeRequest);
    return next;
  }
  function redact(error: unknown, connection: S3Connection) {
    let message = error instanceof Error ? error.message : typeof error === 'string' ? error : '同步失败，请检查连接后重试';
    for (const secret of [connection.accessKeyId, connection.secretAccessKey]) if (secret) {
      for (const value of new Set([secret, encodeURIComponent(secret)])) message = message.replaceAll(value, '[已隐藏]');
    }
    return message;
  }
  const assets = createAssetFetcher(book, async target => {
    if (status.busy) throw new Error('同步正在进行，请稍后下载附件');
    await writes;
    await load();
    if (status.busy) throw new Error('同步正在进行，请稍后下载附件');
    const connection = config;
    const storage = createStore(normalizeSettings(connection));
    if (syncMode(connection) !== 's3' || storage.identity() !== target) throw new Error('请使用原 S3 连接下载附件');
    await connect(connection);
    return storage;
  });
  return {
    fetchAsset: path => assets.fetchAsset(path).catch(error => { throw new Error(redact(error, config)); }),
    load: async () => { await writes.catch(() => {}); await load(); return { ...config }; },
    save,
    flush: () => writes,
    subscribe(listener) { listeners.add(listener); listener(status); return () => { listeners.delete(listener); }; },
    async test(settings) {
      if (status.busy) throw new Error('同步正在进行，请稍候');
      publish({ busy: true, error: false, message: '正在测试 S3 连接…' });
      try {
        await assets.drain();
        await writes;
        await load();
        if (syncMode(config) !== 's3') throw new Error('请先切换至 S3 同步');
        const connection = profile(settings);
        await connect(connection);
        await testS3Connection(connection);
        publish({ message: '连接测试成功：读取、写入、条件写入和删除均正常' });
      } catch (error) {
        const message = redact(error, settings);
        publish({ error: true, message }); throw new Error(message);
      } finally { publish({ busy: false }); }
    },
    async run(settings, accountId) {
      if (status.busy) throw new Error('同步正在进行，请稍候');
      publish({ busy: true, error: false });
      let connection: S3Connection = settings;
      try {
        await assets.drain();
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
          connection = { ...connection, assetDownload: connection.assetDownload ?? defaultAssetDownload(mobile) };
        }
        const next = await connect(connection);
        publish({ message: '正在连接 S3…' });
        const beforeCommit = accountId ? async () => {
          await invoke('account_check_sync', { accountId });
          return prepare();
        } : prepare;
        const synchronize = () => syncNotebook(book, { ...next, ...(connection.assetDownload ? { assetDownload: connection.assetDownload } : {}) },
          message => publish({ message }), beforeCommit);
        const result = await synchronize().catch(reason => {
          if (!(reason instanceof SyncSnapshotChanged)) throw reason;
          return synchronize();
        });
        refresh(); publish({ lastSyncedAt: Date.now(), lastSyncedAccount: accountId, message: `同步完成：${result.notes} 篇笔记、${result.folders} 个文件夹、${result.assets ?? 0} 个附件${result.conflicts ? '，冲突已保留双方' : ''}` });
      } catch (error) {
        if (error instanceof SyncSnapshotChanged) {
          publish({ message: error.message });
          throw error;
        }
        let message = redact(error, connection);
        if (accountId && Object.hasOwn(cloudErrors, message)) message = cloudErrors[message];
        publish({ error: true, message });
        throw new Error(message);
      } finally { publish({ busy: false }); }
    },
  };
}
