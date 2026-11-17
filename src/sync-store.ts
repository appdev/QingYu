import * as core from './vendor/mdsync/core';
export type SyncMode = 'cloud' | 's3';
type S3Fields = Pick<core.Config, 'endpoint' | 'region' | 'bucket' | 'prefix' | 'accessKeyId' | 'secretAccessKey'>;
export type S3Settings = S3Fields & { mode?: SyncMode };
export type S3Connection = S3Fields & Partial<Pick<core.Config, 'forcePathStyle' | 'concurrency'>>;
export function syncMode(settings: S3Settings): SyncMode {
  return settings.mode ?? ([settings.endpoint, settings.bucket, settings.accessKeyId, settings.secretAccessKey].some(value => value?.trim()) ? 's3' : 'cloud');
}
export function normalizeSettings(settings: S3Connection): core.Config {
  const endpoint = new URL(settings.endpoint.trim());
  if (endpoint.protocol !== 'https:' && !(endpoint.protocol === 'http:' && ['127.0.0.1', 'localhost', '[::1]'].includes(endpoint.hostname))) throw new Error('S3 地址需要使用 HTTPS');
  if (endpoint.username || endpoint.password || endpoint.search || endpoint.hash) throw new Error('S3 地址格式无效');
  const bucket = settings.bucket.trim(), prefix = settings.prefix.trim().replace(/^\/+|\/+$/g, '');
  if (!/^[a-z0-9][a-z0-9.-]{1,61}[a-z0-9]$/.test(bucket)) throw new Error('Bucket 名称无效');
  if (!prefix || prefix.split('/').some(part => !/^[A-Za-z0-9._-]+$/.test(part) || part === '.' || part === '..')) throw new Error('请输入独立同步前缀，仅支持字母、数字、点、下划线和短横线');
  if (!settings.accessKeyId.trim() || !settings.secretAccessKey) throw new Error('请填写 S3 访问凭证');
  return { ...core.defaultS3Config(), ...settings, endpoint: endpoint.href.replace(/\/+$/, ''), bucket, prefix, region: settings.region.trim() || 'us-east-1',
    accessKeyId: settings.accessKeyId.trim(), concurrency: settings.concurrency ?? 2, forcePathStyle: settings.forcePathStyle ?? true };
}
export function createStore(config: core.Config): core.Storage {
  const store = core.createS3Adapter(config), sign = core.createS3Signer(config);
  store.putManifest = async (body, etag) => {
    const headers: Record<string, string> = etag === null ? { 'If-None-Match': '*' } : { 'If-Match': etag };
    const signed = await sign({ method: 'PUT', key: `${config.prefix}/${core.REMOTE_MANIFEST_KEY}`, headers, body });
    const reply = await core.requestUrl({ ...signed, method: 'PUT', body: body.slice().buffer as ArrayBuffer });
    if (reply.status === 409 || reply.status === 412) return false;
    if (reply.status < 200 || reply.status >= 300) throw new Error(`S3 条件写入失败（HTTP ${reply.status}）`);
    return true;
  };
  return store;
}
export async function openKey(store: core.Storage) {
  const path = 'miaoyan-key.json';
  let bytes = await store.get(path);
  if (!bytes) {
    if (await store.get(core.REMOTE_MANIFEST_KEY)) throw new Error('远端同步密钥缺失，已停止以保护已有笔记');
    const raw = core.randomBytes(32);
    const body = new TextEncoder().encode(JSON.stringify({ version: 1, protection: 'bucket-permissions', key: btoa(String.fromCharCode(...raw)) }));
    await store.putIfAbsent(path, body, 'application/json');
    bytes = await store.get(path);
  }
  if (!bytes) throw new Error('无法读取同步密钥');
  const value = JSON.parse(new TextDecoder().decode(bytes));
  if (value?.version !== 1 || value.protection !== 'bucket-permissions' || typeof value.key !== 'string') throw new Error('远端同步密钥格式无效');
  const raw = Uint8Array.from(atob(value.key), char => char.charCodeAt(0));
  if (raw.length !== 32) throw new Error('远端同步密钥格式无效');
  return core.importAesKey(raw);
}
