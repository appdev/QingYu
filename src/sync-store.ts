import * as core from './vendor/mdsync/core';
export type SyncMode = 'cloud' | 's3';
type S3Fields = Pick<core.Config, 'endpoint' | 'region' | 'bucket' | 'prefix' | 'accessKeyId' | 'secretAccessKey'>;
type S3Request = (input: core.S3Request) => Promise<core.Response>;
export type CloudUpload = { configVersion: number; storageGeneration?: number; key: string; ifMatch?: string; ifNoneMatch?: string };
type CloudWrites = {
  upload(object: CloudUpload, bytes: Uint8Array): Promise<number>;
  delete(configVersion: number, keys: string[], generation?: number): Promise<void>;
};
type CloudVault = CloudWrites & { configVersion: number; repository: string; prefix: string; storageGeneration?: number };
export type S3Connection = S3Fields & Partial<Pick<core.Config, 'forcePathStyle' | 'concurrency'>> & {
  timeout?: number; tlsVerify?: boolean; assetDownload?: 'all' | 'on-demand'; request?: S3Request; cloud?: CloudVault;
  canUpload?: boolean; accountId?: string;
};
export type S3Grant = S3Connection & { credentialsExpireAt: number } & (
  { protocolVersion?: undefined } |
  { protocolVersion: 2; configVersion: number; provider: string; repository: string; sessionToken: string; storageGeneration?: number }
);
export type S3Config = core.Config & Pick<S3Connection, 'cloud' | 'request' | 'canUpload' | 'accountId'>;
export type S3Settings = Omit<S3Connection, 'cloud' | 'request' | 'canUpload' | 'accountId'> & { mode?: SyncMode };
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
function normalizeTarget(settings: S3Connection): S3Config {
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
export function normalizeSettings(settings: S3Connection): S3Config {
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
export function withCloudCredentials(grant: S3Grant, refresh: (rejectedKey?: string) => Promise<S3Grant>, writes?: CloudWrites): S3Connection {
  const cloud = grant.protocolVersion === 2 && writes ? {
    ...writes, configVersion: grant.configVersion, repository: grant.repository, prefix: grant.prefix, storageGeneration: grant.storageGeneration,
  } : undefined;
  if (grant.protocolVersion === 2 && !cloud) throw new Error('invalid_response');
  const vault = (config: S3Grant) => cloud ? { ...config, prefix: `${cloud.prefix}vault-v2`, cloud } : config;
  const target = normalizeSettings(vault(grant));
  let current = grant, sign = core.createS3Signer(target);
  let pending: Promise<void> | undefined;
  function renew(rejectedKey?: string) {
    return pending ??= refresh(rejectedKey).then(next => {
      if (next.protocolVersion !== grant.protocolVersion || (grant.protocolVersion === 2 &&
          (next.protocolVersion !== 2 || next.prefix !== grant.prefix || next.configVersion !== grant.configVersion ||
           next.provider !== grant.provider || next.repository !== grant.repository ||
           next.storageGeneration !== grant.storageGeneration))) throw new Error('storage_config_changed');
      const config = normalizeSettings(vault(next));
      for (const key of ['endpoint', 'bucket', 'prefix', 'region', 'forcePathStyle'] as const) {
        if (config[key] !== target[key]) throw new Error('轻羽存储目标已变更，请重新同步');
      }
      current = next;
      sign = core.createS3Signer(config);
    }).finally(() => { pending = undefined; });
  }
  async function send(input: core.S3Request) {
    const grant = current, signer = sign;
    if (grant.protocolVersion === 2) input = {
      ...input,
      key: await cloudObjectKey(target, input.key),
      headers: { ...input.headers, 'x-amz-security-token': grant.sessionToken },
    };
    const signed = await signer(input);
    const reply = await core.requestUrl({ ...signed, method: input.method, body: input.body?.slice().buffer, storageGeneration: cloud?.storageGeneration });
    if (reply.status < 400) return { grant, reply };
    const code = /<Code>\s*([A-Za-z0-9_]+)\s*<\/Code>/.exec(reply.text)?.[1];
    let text = grant.protocolVersion === 2 ? (code ? `<Code>${code}</Code>` : 'S3 request failed') : reply.text;
    for (const secret of [grant.accessKeyId, grant.secretAccessKey, grant.protocolVersion === 2 ? grant.sessionToken : ''].filter(Boolean)) {
      text = text.replaceAll(secret, '[已隐藏]').replaceAll(encodeURIComponent(secret), '[已隐藏]');
    }
    return { grant, reply: { ...reply, text } };
  }
  return { ...vault(grant), async request(input) {
    if (cloud && input.method !== 'GET' && input.method !== 'HEAD') throw new Error('invalid_request');
    if (pending || current.credentialsExpireAt * 1000 - Date.now() <= 120_000) await (pending ?? renew());
    const sent = await send(input);
    let reply = sent.reply;
    if ([400, 401, 403].includes(reply.status) &&
        /<Code>\s*(?:ExpiredToken|InvalidToken|InvalidAccessKeyId|InvalidSecurityToken|SecurityTokenExpired)\s*<\/Code>/.test(reply.text)) {
      if (pending || current === sent.grant) await (pending ?? renew(sent.grant.accessKeyId));
      reply = (await send(input)).reply;
    }
    if (cloud && input.method === 'GET' && input.key && reply.status === 403 && reply.text === '<Code>AccessDenied</Code>') {
      // With prefix-scoped ListBucket, S3 can return 403 for an object that does not exist.
      const prefix = await cloudObjectKey(target, input.key);
      const listing = (await send({ method: 'GET', key: '', query: { 'list-type': '2', prefix, 'max-keys': '1' } })).reply;
      if (listing.status === 200 && /<IsTruncated>\s*false\s*<\/IsTruncated>/.test(listing.text)) {
        const page = core.parseListObjects(listing.text);
        if (!page.nextToken && !page.objects.length) return { ...reply, status: 404, text: '<Code>NoSuchKey</Code>' };
      }
    }
    return reply;
  } };
}

async function cloudObjectKey(config: S3Config, key: string) {
  if (!config.cloud || !key) return key;
  if (key === `${config.prefix}/miaoyan-key.json`) return `${config.cloud.prefix}miaoyan-key.json`;
  const history = `${config.prefix}/history/`;
  if (key.startsWith(history) && key.slice(history.length).includes('/')) {
    return history + await core.sha256Hex(new TextEncoder().encode(key.slice(history.length))) + '.json.enc';
  }
  return key;
}

async function cloudPut(config: S3Config, path: string, body: Uint8Array, condition?: { etag: string | null }) {
  if (config.canUpload === false) throw new Error('storage_expired');
  const cloud = config.cloud!;
  const key = await cloudObjectKey(config, `${config.prefix}/${path}`);
  const status = await cloud.upload({ configVersion: cloud.configVersion, key, storageGeneration: cloud.storageGeneration,
    ...(condition ? (condition.etag === null ? { ifNoneMatch: '*' } : { ifMatch: condition.etag }) : {}),
  }, body);
  if (condition && (status === 409 || status === 412)) return false;
  if (status < 200 || status >= 300) throw new Error(`S3 写入失败（HTTP ${status}）`);
  return true;
}

export function createStore(config: S3Config): core.Storage {
  const store = core.createS3Adapter(config, config.request), sign = core.createS3Signer(config);
  if (config.cloud) {
    const cloud = config.cloud;
    store.maxWholeObjectBytes = 256 * 1024;
    store.keyCacheIdentity = JSON.stringify(['qingyu-v2', config.accountId, cloud.repository]);
    store.put = async (path, body) => { await cloudPut(config, path, body); };
    store.putIfAbsent = (path, body) => cloudPut(config, path, body, { etag: null });
    store.delete = async path => {
      const keys = [await cloudObjectKey(config, `${config.prefix}/${path}`)];
      if (cloud.storageGeneration === undefined) await cloud.delete(cloud.configVersion, keys);
      else await cloud.delete(cloud.configVersion, keys, cloud.storageGeneration);
    };
  }
  store.putManifest = (body, etag) => putSyncObject(config, core.REMOTE_MANIFEST_KEY, body, etag, sign);
  return store;
}
export async function putSyncObject(config: S3Config, path: string, body: Uint8Array, etag: string | null, sign = core.createS3Signer(config)) {
  if (config.cloud) return cloudPut(config, path, body, { etag });
  const headers: Record<string, string> = etag === null ? { 'If-None-Match': '*' } : { 'If-Match': etag };
  const input = { method: 'PUT', key: `${config.prefix}/${path}`, headers, body };
  const reply = config.request ? await config.request(input)
    : await core.requestUrl({ ...await sign(input), method: 'PUT', body: body.slice().buffer as ArrayBuffer });
  if (reply.status === 409 || reply.status === 412) return false;
  if (reply.status < 200 || reply.status >= 300) throw new Error(`S3 条件写入失败（HTTP ${reply.status}）`);
  return true;
}

const chunkKeys = new WeakMap<CryptoKey, CryptoKey>();
export async function importSyncKey(raw: Uint8Array) {
  const key = await core.importAesKey(raw);
  chunkKeys.set(key, await core.deriveChunkIds(raw));
  return key;
}
export function syncChunkIds(key: CryptoKey) { return chunkKeys.get(key); }
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
  if (value?.version === 2 && value.protection === 'password') throw new Error('该同步库受 App 数据仓库密钥保护，Web 部署端请使用独立同步前缀');
  return importBucketKey(value);
}

export async function importBucketKey(value: { version?: number; protection?: string; key?: unknown } | null) {
  if (value?.version !== 1 || value.protection !== 'bucket-permissions' || typeof value.key !== 'string') throw new Error('远端同步密钥格式无效');
  const raw = Uint8Array.from(atob(value.key), char => char.charCodeAt(0));
  if (raw.length !== 32) throw new Error('远端同步密钥格式无效');
  return importSyncKey(raw);
}
