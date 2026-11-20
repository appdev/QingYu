import * as core from './vendor/mdsync/core';
export type SyncMode = 'cloud' | 's3';
type S3Fields = Pick<core.Config, 'endpoint' | 'region' | 'bucket' | 'prefix' | 'accessKeyId' | 'secretAccessKey'>;
export type S3Connection = S3Fields & Partial<Pick<core.Config, 'forcePathStyle' | 'concurrency'>> & { timeout?: number; tlsVerify?: boolean; assetDownload?: 'all' | 'on-demand' };
export type S3Settings = S3Connection & { mode?: SyncMode };
export const s3Defaults = { timeout: 45, tlsVerify: true, forcePathStyle: true, concurrency: 2, assetDownload: 'all' as const };
export const defaultAssetDownload = (mobile: boolean) => mobile ? 'on-demand' as const : s3Defaults.assetDownload;
export function hasS3Connection(settings: S3Settings) {
  return [settings.endpoint, settings.bucket, settings.prefix, settings.accessKeyId, settings.secretAccessKey].every(value => value?.trim());
}
export function validateS3Options(settings: S3Connection) {
  if (settings.assetDownload !== undefined && settings.assetDownload !== 'all' && settings.assetDownload !== 'on-demand') throw new Error('附件下载设置无效');
  if (settings.timeout !== undefined && (!Number.isInteger(settings.timeout) || settings.timeout < 1 || settings.timeout > 300)) throw new Error('超时需要为 1–300 秒的整数');
  if (settings.concurrency !== undefined && (!Number.isInteger(settings.concurrency) || settings.concurrency < 1 || settings.concurrency > 16)) throw new Error('并发请求数需要为 1–16 的整数');
  if (settings.forcePathStyle !== undefined && typeof settings.forcePathStyle !== 'boolean' || settings.tlsVerify !== undefined && typeof settings.tlsVerify !== 'boolean') throw new Error('寻址方式或 TLS 校验设置无效');
}
export function syncMode(settings: S3Settings): SyncMode {
  return settings.mode ?? ([settings.endpoint, settings.bucket, settings.accessKeyId, settings.secretAccessKey].some(value => value?.trim()) ? 's3' : 'cloud');
}
function normalizeTarget(settings: S3Connection): core.Config {
  validateS3Options(settings);
  const endpoint = new URL(settings.endpoint.trim());
  if (endpoint.protocol !== 'https:' && !(endpoint.protocol === 'http:' && ['127.0.0.1', 'localhost', '[::1]'].includes(endpoint.hostname))) throw new Error('S3 地址需要使用 HTTPS');
  if (endpoint.username || endpoint.password || endpoint.search || endpoint.hash) throw new Error('S3 地址格式无效');
  const bucket = settings.bucket.trim(), prefix = settings.prefix.trim().replace(/^\/+|\/+$/g, '');
  if (!/^[a-z0-9][a-z0-9.-]{1,61}[a-z0-9]$/.test(bucket)) throw new Error('Bucket 名称无效');
  if (!prefix || prefix.split('/').some(part => !/^[A-Za-z0-9._-]+$/.test(part) || part === '.' || part === '..')) throw new Error('请输入独立同步前缀，仅支持字母、数字、点、下划线和短横线');
  return { ...core.defaultS3Config(), ...settings, endpoint: endpoint.href.replace(/\/+$/, ''), bucket, prefix, region: settings.region.trim() || 'us-east-1',
    accessKeyId: settings.accessKeyId.trim(), concurrency: settings.concurrency ?? 2, forcePathStyle: settings.forcePathStyle ?? true };
}
export function normalizeSettings(settings: S3Connection): core.Config {
  const config = normalizeTarget(settings);
  if (!config.accessKeyId || !config.secretAccessKey) throw new Error('请填写 S3 访问凭证');
  return config;
}
export function syncTargetIdentity(settings: S3Connection) {
  return core.createS3Adapter(normalizeTarget(settings)).identity();
}
export async function testS3Connection(settings: S3Connection) {
  const config = normalizeSettings(settings), sign = core.createS3Signer(config);
  const key = `${config.prefix}/.connection-test/${crypto.randomUUID()}`;
  const bytes = core.randomBytes(32), body = bytes.slice().buffer as ArrayBuffer;
  async function request(method: string, headers: Record<string, string> = {}, content?: ArrayBuffer) {
    const signed = await sign({ method, key, headers, body: content ? new Uint8Array(content) : undefined });
    return core.requestUrl({ ...signed, method, body: content });
  }
  function check(reply: core.Response, operation: string) {
    if (reply.status < 200 || reply.status >= 300) throw new Error(`${operation}失败（HTTP ${reply.status}）\n${reply.text.slice(0, 4096)}`.trim());
  }
  const failures: unknown[] = [];
  try {
    check(await request('PUT', { 'If-None-Match': '*' }, body), '写入测试');
    const read = await request('GET'); check(read, '读取测试');
    if (await core.sha256Hex(new Uint8Array(read.arrayBuffer)) !== await core.sha256Hex(bytes)) throw new Error('读取测试失败：返回内容与写入内容不一致');
    const etag = Object.entries(read.headers).find(([name]) => name.toLowerCase() === 'etag')?.[1];
    if (!etag) throw new Error('连接测试失败：服务未返回 ETag，无法安全同步');
    const rejected = await request('PUT', { 'If-Match': '"qingyu-stale-test"' }, body);
    if (rejected.status !== 412) throw new Error(`条件写入测试失败：预期 HTTP 412，实际 HTTP ${rejected.status}\n${rejected.text.slice(0, 4096)}`.trim());
    check(await request('PUT', { 'If-Match': etag }, body), '条件写入测试');
  } catch (error) { failures.push(error); }
  try { check(await request('DELETE'), '清理测试文件'); }
  catch (error) { failures.push(error); }
  if (failures.length) throw new Error(failures.map(error => error instanceof Error ? error.message : String(error)).join('\n'));
}
export function createStore(config: core.Config): core.Storage {
  const store = core.createS3Adapter(config), sign = core.createS3Signer(config);
  store.putManifest = (body, etag) => putSyncObject(config, core.REMOTE_MANIFEST_KEY, body, etag, sign);
  return store;
}
export async function putSyncObject(config: core.Config, path: string, body: Uint8Array, etag: string | null, sign = core.createS3Signer(config)) {
  const headers: Record<string, string> = etag === null ? { 'If-None-Match': '*' } : { 'If-Match': etag };
  const signed = await sign({ method: 'PUT', key: `${config.prefix}/${path}`, headers, body });
  const reply = await core.requestUrl({ ...signed, method: 'PUT', body: body.slice().buffer as ArrayBuffer });
  if (reply.status === 409 || reply.status === 412) return false;
  if (reply.status < 200 || reply.status >= 300) throw new Error(`S3 条件写入失败（HTTP ${reply.status}）`);
  return true;
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
  if (value?.version === 2 && value.protection === 'password') throw new Error('该同步库受 App 同步密码保护，Web 部署端请使用独立同步前缀');
  return importBucketKey(value);
}

export async function importBucketKey(value: { version?: number; protection?: string; key?: unknown } | null) {
  if (value?.version !== 1 || value.protection !== 'bucket-permissions' || typeof value.key !== 'string') throw new Error('远端同步密钥格式无效');
  const raw = Uint8Array.from(atob(value.key), char => char.charCodeAt(0));
  if (raw.length !== 32) throw new Error('远端同步密钥格式无效');
  return core.importAesKey(raw);
}
