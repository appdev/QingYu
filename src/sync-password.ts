import { createStore, importBucketKey, importSyncKey, putSyncObject, type S3Config } from './sync-store';
import { REMOTE_MANIFEST_KEY, sha256Hex, type Storage } from './vendor/mdsync/core';

const path = 'miaoyan-key.json';
const encoder = new TextEncoder();
const iterations = 600_000;
const kdf = { name: 'PBKDF2-SHA256', iterations } as const;
export const syncPasswordRequired = '需要输入数据仓库密钥';
export class LegacySyncKey extends Error {
  constructor() { super('该同步库使用旧密钥格式，请使用新的同步前缀'); }
}
const invalidEnvelope = '远端同步密钥文件格式无效';
export type PasswordState = 'new' | 'locked' | 'unlocked';
export type PasswordInfo = { state: PasswordState; etag: string | null; immutable?: true };
export type PasswordAction = PasswordInfo & { password: string };
export type KeyCache = (target: string, key?: string) => Promise<string | null>;

const encode = (bytes: Uint8Array) => btoa(String.fromCharCode(...bytes));
function decode(value: unknown, size: number) {
  if (typeof value !== 'string' || value.length !== Math.ceil(size / 3) * 4) throw new Error(invalidEnvelope);
  const bytes = Uint8Array.from(atob(value), char => char.charCodeAt(0));
  if (bytes.length !== size || encode(bytes) !== value) throw new Error(invalidEnvelope);
  return bytes;
}

async function readEnvelope(store: Storage) {
  const result = await store.getIfChanged!(path, null);
  if (result.status === 'absent') {
    if (await store.get(REMOTE_MANIFEST_KEY)) throw new Error('远端同步密钥缺失，已停止以保护已有笔记');
    return null;
  }
  if (result.status !== 'found' || result.body.length > 4096) throw new Error(invalidEnvelope);
  let value;
  try { value = JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(result.body)); }
  catch { throw new Error(invalidEnvelope); }
  if (value?.version === 1) return { legacy: value };
  try {
    if (value?.version !== 2 || value.protection !== 'password' || typeof value.keyId !== 'string' ||
        !/^[0-9a-f]{64}$/.test(value.keyId) || value.kdf?.name !== kdf.name || value.kdf.iterations !== iterations ||
        (value.kdf.normalization !== undefined && value.kdf.normalization !== 'NFC')) {
      throw new Error(invalidEnvelope);
    }
    return { keyId: value.keyId as string, salt: decode(value.kdf.salt, 16),
      nonce: decode(value.nonce, 12), wrapped: decode(value.wrapped, 48), etag: result.etag,
      normalization: value.kdf.normalization as 'NFC' | undefined };
  } catch {
    throw new Error(invalidEnvelope);
  }
}

async function cachedKey(store: Storage, cache: KeyCache, keyId: string) {
  const identity = store.keyCacheIdentity ?? store.identity();
  for (const target of new Set([identity, store.identity()])) {
    const saved = await cache(target);
    if (!saved) continue;
    const raw = decode(saved, 32);
    if (await sha256Hex(raw) !== keyId) continue;
    if (target !== identity) await cache(identity, saved);
    return raw;
  }
  return null;
}

export async function openProtectedKey(store: Storage, cache: KeyCache, purpose: 'sync' | 'attachment' = 'sync') {
  const envelope = await readEnvelope(store);
  if (envelope && 'legacy' in envelope) {
    if (purpose === 'attachment') return importBucketKey(envelope.legacy);
    throw new LegacySyncKey();
  }
  const raw = envelope && await cachedKey(store, cache, envelope.keyId);
  if (!raw) throw new Error(syncPasswordRequired);
  return importSyncKey(raw);
}

async function deriveKey(password: Uint8Array<ArrayBuffer>, salt: Uint8Array<ArrayBuffer>) {
  const material = await crypto.subtle.importKey('raw', password, 'PBKDF2', false, ['deriveKey']);
  return crypto.subtle.deriveKey({ name: 'PBKDF2', hash: 'SHA-256', salt, iterations },
    material, { name: 'AES-GCM', length: 256 }, false, ['encrypt', 'decrypt']);
}

function gcm(prefix: string, keyId: string, iv: Uint8Array<ArrayBuffer>, normalization?: 'NFC'): AesGcmParams {
  const fields = [path, 2, prefix, keyId];
  if (normalization) fields.push(normalization);
  return { name: 'AES-GCM', iv, tagLength: 128, additionalData: encoder.encode(JSON.stringify(fields)) };
}

export async function manageSyncPassword(config: S3Config, cache: KeyCache, action?: PasswordAction, beforeWrite?: () => Promise<void>): Promise<PasswordInfo> {
  const store = createStore(config);
  const envelope = await readEnvelope(store);
  if (envelope && 'legacy' in envelope) throw new LegacySyncKey();
  const cached = envelope && await cachedKey(store, cache, envelope.keyId);
  const state: PasswordState = !envelope ? 'new' : cached ? 'unlocked' : 'locked';
  const etag = envelope?.etag ?? null;
  const policy = config.cloud ? { immutable: true as const } : {};
  if (!action) return { state, etag, ...policy };
  if (state !== action.state || etag !== action.etag) throw new Error('数据仓库密钥状态已变更，请重新打开密钥设置');
  if (config.cloud && state === 'unlocked') throw new Error('云存储密钥设定后不可修改');
  const normalization = state === 'locked' ? envelope?.normalization : 'NFC';
  const input = normalization ? action.password.normalize(normalization) : action.password;
  const password = encoder.encode(input);
  if (!password.length) throw new Error('请输入数据仓库密钥');
  const repository = config.cloud?.repository ?? config.prefix;
  const identity = store.keyCacheIdentity ?? store.identity();
  let raw: Uint8Array<ArrayBuffer>;
  if (envelope && !cached) {
    const kek = await deriveKey(password, envelope.salt);
    try {
      raw = new Uint8Array(await crypto.subtle.decrypt(gcm(repository, envelope.keyId, envelope.nonce, normalization), kek, envelope.wrapped));
    } catch {
      throw new Error('数据仓库密钥错误或密钥文件已损坏');
    }
    if (raw.length !== 32 || await sha256Hex(raw) !== envelope.keyId) throw new Error(invalidEnvelope);
  } else {
    if (envelope && !envelope.etag) throw new Error('服务未返回 ETag，无法安全修改数据仓库密钥');
    const previous = store.keyCacheIdentity && await cache(identity);
    raw = cached ?? (previous ? decode(previous, 32) : crypto.getRandomValues(new Uint8Array(32)));
    const keyId = await sha256Hex(raw);
    const salt = crypto.getRandomValues(new Uint8Array(16)), nonce = crypto.getRandomValues(new Uint8Array(12));
    const kek = await deriveKey(password, salt);
    const wrapped = await crypto.subtle.encrypt(gcm(repository, keyId, nonce, normalization), kek, raw);
    const body = encoder.encode(JSON.stringify({ version: 2, protection: 'password', keyId,
      kdf: { ...kdf, salt: encode(salt), normalization }, nonce: encode(nonce), wrapped: encode(new Uint8Array(wrapped)) }));
    await beforeWrite?.();
    if (!await putSyncObject(config, path, body, etag)) throw new Error('数据仓库密钥状态已变更，请重新打开密钥设置');
  }
  if (!cached) await cache(identity, encode(raw));
  return { state: 'unlocked', etag: null, ...policy };
}
