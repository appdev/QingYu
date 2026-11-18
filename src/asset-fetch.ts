import type { Notebook } from './notebook';
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
