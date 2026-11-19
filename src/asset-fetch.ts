import type { Notebook } from './notebook';
import { emitTo, listen } from '@tauri-apps/api/event';
import { assetPath, assetRelative } from './sync-files';
import { openKey } from './sync-store';
import { loadRemoteBytes, sha256Hex, type Storage } from './vendor/mdsync/core';

export async function downloadAsset(
  storage: Storage, hash: string, size: number,
  load: typeof loadRemoteBytes = loadRemoteBytes,
) {
  const key = await openKey(storage);
  const bytes = await load({ storage, key }, hash, size);
  if (!bytes || bytes.length !== size || await sha256Hex(bytes) !== hash) throw new Error('附件内容校验失败');
  return bytes;
}

export function createAssetFetcher(book: Notebook, connect: (target: string) => Promise<Storage>) {
  const pending = new Map<string, Promise<string | undefined>>();
  const waiting: (() => void)[] = [];
  let active = 0;
  function pump() {
    while (active < 3 && waiting.length) {
      active++;
      waiting.shift()!();
    }
  }
  function fetchAsset(path: string): Promise<string | undefined> {
    const root = book.root;
    if (!root || book.external || book.data.sync?.version !== 2) return Promise.resolve(undefined);
    const target = book.data.sync.target;
    const id = JSON.stringify([root, path]);
    const existing = pending.get(id);
    if (existing) return existing;
    const job = new Promise<string | undefined>((resolve, reject) => {
      waiting.push(() => {
        void (async () => {
          if (book.root !== root || book.data.sync?.target !== target) throw new Error('笔记库已切换，请重新下载附件');
          const key = assetPath(path);
          assetRelative(key);
          const entries = await book.readAssetPlaceholders();
          if (!Object.hasOwn(entries, path)) return;
          const entry = entries[path]!;
          const base = book.data.sync?.state.baseline?.files[key];
          if (!base || entry.hash !== base.hash || entry.size !== base.size || entry.mtime !== base.mtime) {
            throw new Error('附件占位与同步基线不匹配');
          }
          const bytes = await downloadAsset(await connect(target), entry.hash, entry.size);
          await book.materializeAsset(root, path, entry.hash, bytes);
          return entry.hash;
        })().then(resolve, reject);
      });
    }).finally(() => {
      pending.delete(id);
      active--;
      pump();
    });
    pending.set(id, job);
    pump();
    return job;
  }
  return { fetchAsset, drain: () => Promise.allSettled(pending.values()) };
}

type WindowRequest = { id: string; origin: string; root: string; path: string };
type WindowReply = { hash?: string; error?: string };
const assetRequestEvent = 'asset-download-request';

export function hostAssetDownloads(book: Notebook, fetchAsset: (path: string) => Promise<string | undefined>) {
  return listen<WindowRequest>(assetRequestEvent, async ({ payload }) => {
    const { id, origin, root, path } = payload ?? {};
    if (typeof id !== 'string' || !/^[a-f0-9-]{36}$/.test(id) || typeof origin !== 'string' || !origin.startsWith('document-note-')
        || typeof path !== 'string' || path.length > 4096 || typeof root !== 'string') return;
    let reply: WindowReply;
    try {
      if (book.root !== root) throw new Error('笔记库已切换，请重新下载附件');
      reply = { hash: await fetchAsset(path) };
    } catch (error) {
      reply = { error: error instanceof Error ? error.message : '附件下载失败' };
    }
    await emitTo(origin, `asset-download-reply-${id}`, reply);
  }, { target: 'main' });
}

export async function downloadFromMain(origin: string, root: string, path: string) {
  const id = crypto.randomUUID();
  let receive!: (reply: WindowReply) => void;
  const response = new Promise<WindowReply>(resolve => { receive = resolve; });
  const stop = await listen<WindowReply>(`asset-download-reply-${id}`, ({ payload }) => receive(payload), { target: origin });
  const timer = setTimeout(() => receive({ error: '主窗口未响应，请打开主窗口后重试' }), 300_000);
  try {
    await emitTo('main', assetRequestEvent, { id, origin, root, path } satisfies WindowRequest);
    const reply = await response;
    if (reply.error) throw new Error(reply.error);
    return reply.hash;
  } finally {
    clearTimeout(timer);
    stop();
  }
}
