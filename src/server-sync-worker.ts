import { readFile } from 'node:fs/promises';
import { request as httpsRequest } from 'node:https';
import { createInterface } from 'node:readline';
import { SyncSnapshotChanged, type Library } from './notebook';
import type { AssetPlaceholders, AssetStat, StagedAsset } from './sync-files';
import { syncNotebook } from './sync';
import { normalizeSettings, s3Defaults, testS3Connection, withCloudCredentials, type S3Connection, type S3Grant } from './sync-store';
import { setRequestHandler } from './vendor/mdsync/core';

// Only the private server process owns these pipes. Browsers never run this entry point.
const input = createInterface({ input: process.stdin, crlfDelay: Infinity });
const replies = input[Symbol.asyncIterator]();
let pending = Promise.resolve();
function request<T>(operation: string, args: object = {}): Promise<T> {
  const result = pending.then(async () => {
    process.stdout.write(JSON.stringify({ operation, ...args }) + '\n');
    const reply = await replies.next();
    if (reply.done) throw new Error('服务器同步会话已关闭');
    const value = JSON.parse(reply.value);
    if (operation === 'credentials' && value.credentialError) throw new Error(value.credentialError);
    if (value.error === 'stale') throw new SyncSnapshotChanged();
    if (value.error) throw new Error('服务器笔记库未能处理同步请求');
    return value.value as T;
  });
  pending = result.then(() => undefined, () => undefined);
  return result;
}

const MAX_RESPONSE = 68 * 1024 * 1024;
// 报错前要隐藏的连接凭据，和 App 的同步错误处理一致
const secrets: string[] = [];

type Reply = { status: number; headers: Record<string, string>; bytes: Uint8Array<ArrayBuffer> };

function join(chunks: Uint8Array[], size: number) {
  const bytes = new Uint8Array(size);
  let offset = 0;
  for (const chunk of chunks) { bytes.set(chunk, offset); offset += chunk.length; }
  return bytes;
}

async function requestWithFetch(url: string, method: string, headers: Record<string, string> | undefined, body: ArrayBuffer | undefined, timeout: number): Promise<Reply> {
  const response = await fetch(url, { method, body, headers, redirect: 'error', signal: AbortSignal.timeout(timeout) });
  const chunks: Uint8Array[] = [];
  let size = 0;
  const reader = response.body?.getReader();
  if (reader) {
    try {
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        size += value.length;
        if (size > MAX_RESPONSE) { await reader.cancel(); throw new Error('S3 响应过大'); }
        chunks.push(value);
      }
    } finally { reader.releaseLock(); }
  }
  return { status: response.status, headers: Object.fromEntries(response.headers), bytes: join(chunks, size) };
}

function requestWithoutVerification(url: string, method: string, headers: Record<string, string> | undefined, body: ArrayBuffer | undefined, timeout: number): Promise<Reply> {
  return new Promise((resolve, reject) => {
    const request = httpsRequest(url, { method, headers, rejectUnauthorized: false, timeout }, response => {
      const chunks: Uint8Array[] = [];
      let size = 0;
      response.on('data', (chunk: Buffer) => {
        size += chunk.length;
        if (size > MAX_RESPONSE) { response.destroy(new Error('S3 响应过大')); return; }
        chunks.push(chunk);
      });
      response.on('error', reject);
      response.on('end', () => {
        const replyHeaders: Record<string, string> = {};
        for (const [key, value] of Object.entries(response.headers)) if (value !== undefined) replyHeaders[key] = Array.isArray(value) ? value.join(', ') : value;
        resolve({ status: response.statusCode ?? 0, headers: replyHeaders, bytes: join(chunks, size) });
      });
    });
    request.on('timeout', () => request.destroy(new Error('S3 请求超时')));
    request.on('error', reject);
    request.end(body ? Buffer.from(body) : undefined);
  });
}

async function run() {
  const saved = JSON.parse(await readFile(process.argv[2]!, 'utf8')) as S3Connection & { mode?: string };
  if (saved.mode && saved.mode !== 's3' && saved.mode !== 'cloud') throw new Error('请先切换至 S3 同步');
  async function credentials(rejectedKey?: string) {
    const { concurrentReqs, ...grant } = await request<S3Grant & { concurrentReqs: number }>('credentials', { rejectedKey });
    return { ...grant, concurrency: Math.max(1, Math.min(concurrentReqs || 2, 8)) };
  }
  const profile = saved.mode === 'cloud' ? withCloudCredentials(await credentials(), credentials) : saved;
  secrets.push(...[profile.accessKeyId, profile.secretAccessKey].filter(Boolean));
  const config = normalizeSettings(profile);
  const target = new URL(config.endpoint);
  if (!config.forcePathStyle) target.hostname = config.bucket + '.' + target.hostname;
  const timeout = (profile.timeout ?? s3Defaults.timeout) * 1000;
  // 和 App 一样支持在设置里跳过证书校验；Node 的 fetch 不能单独关闭校验，这种情况改用 https 模块
  const insecure = profile.tlsVerify === false;
  setRequestHandler(async ({ url, method, body, headers }) => {
    if (new URL(url).origin !== target.origin) throw new Error('同步请求超出目标范围');
    const { status, headers: replyHeaders, bytes } = insecure
      ? await requestWithoutVerification(url, method, headers, body, timeout)
      : await requestWithFetch(url, method, headers, body, timeout);
    return { status, headers: replyHeaders, arrayBuffer: bytes.buffer,
      get text() { return new TextDecoder().decode(bytes); }, json: {} };
  });

  if (process.argv[3] === 'test') { await testS3Connection(profile); return; }
  async function synchronize() {
    const { library, revision, assets, placeholders } = await request<{
      library: Library; revision: number; assets: AssetStat[]; placeholders: AssetPlaceholders;
    }>('snapshot');
    const bytes = async (operation: string, args: object) => Buffer.from(await request<string>(operation, args), 'base64');
    return syncNotebook({
      data: library, loadError: false, root: undefined, assetRevision: revision,
      async readSyncAssets() { return assets; },
      async readAssetPlaceholders() { return placeholders; },
      readAsset: asset => bytes('readAsset', { asset, revision }),
      stageAsset: (_batch, data) => request<StagedAsset>('stage', { data: Buffer.from(data).toString('base64'), revision }),
      readStagedAsset: (_batch, asset) => bytes('readStage', { asset }),
      async releaseAssetStage() { await request('release'); },
      commitSync: async (next, sync, expected, changes, _location, published) => {
        await request('commit', { revision, next: { ...next, sync }, changes, published, expectedSync: expected.sync });
      },
    }, { ...config, assetDownload: 'all' });
  }
  let result: Awaited<ReturnType<typeof synchronize>>;
  try { result = await synchronize(); }
  catch (reason) { if (!(reason instanceof SyncSnapshotChanged)) throw reason; result = await synchronize(); }
  // 和 App 同步完成时的提示相同
  await request('finished', { message: `同步完成：${result.notes} 篇笔记、${result.folders} 个文件夹、${result.assets ?? 0} 个附件${result.conflicts ? '，冲突已保留双方' : ''}` });
}

try { await run(); }
catch (reason) {
  process.exitCode = 1;
  // 把失败原因交给 qingyu-api，设置页和 App 一样显示具体错误
  let message = reason instanceof Error ? reason.message : String(reason);
  for (const secret of secrets) for (const value of new Set([secret, encodeURIComponent(secret)])) message = message.replaceAll(value, '[已隐藏]');
  await request('failed', { message: message.slice(0, 4096) }).catch(() => {});
}
finally { input.close(); process.stdin.destroy(); }
