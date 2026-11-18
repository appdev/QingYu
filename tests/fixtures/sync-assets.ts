import { expect, vi } from 'vitest';
import type { Notebook } from '../../src/notebook';
import type { Asset, AssetPlaceholders } from '../../src/sync-files';
import { sha256Hex } from '../../src/vendor/mdsync/core';

export function mockAssetDisk(book: Notebook, initial: Asset[] = []) {
  let assets = structuredClone(initial);
  let placeholders: AssetPlaceholders = {};
  const stages = new Map<string, Uint8Array>();
  const metadata = () => assets.map(({ path, bytes, modified }) => ({ path, size: bytes.length, modified }));
  vi.spyOn(book, 'readSyncAssets').mockImplementation(async () => metadata());
  vi.spyOn(book, 'readAssetPlaceholders').mockImplementation(async () => structuredClone(placeholders));
  const reads = vi.spyOn(book, 'readAsset').mockImplementation(async ({ path }) => {
    const asset = assets.find(asset => asset.path === path);
    if (!asset) throw new Error('附件不存在');
    return Uint8Array.from(asset.bytes);
  });
  vi.spyOn(book, 'stageAsset').mockImplementation(async (batch, bytes) => {
    const path = `${batch}/${crypto.randomUUID()}`;
    stages.set(path, bytes.slice());
    return { path, hash: await sha256Hex(bytes) };
  });
  vi.spyOn(book, 'readStagedAsset').mockImplementation(async (_batch, { path }) => {
    const bytes = stages.get(path);
    if (!bytes) throw new Error('暂存附件不存在');
    return bytes;
  });
  vi.spyOn(book, 'releaseAssetStage').mockImplementation(async batch => {
    for (const key of stages.keys()) if (key.startsWith(`${batch}/`)) stages.delete(key);
  });
  function removePlaceholder(path: string, hash: string) {
    if (placeholders[path]?.hash !== hash || assets.some(asset => asset.path === path)) throw new Error('附件占位已变化');
    delete placeholders[path];
    const { target, state } = book.data.sync!;
    const placeholderCount = Object.keys(placeholders).length;
    book.data.sync = placeholderCount ? { target, state, version: 2, placeholderCount } : { target, state, version: 1 };
  }
  vi.spyOn(book, 'materializeAsset').mockImplementation(async (_root, path, hash, bytes) => {
    const entry = placeholders[path]!;
    removePlaceholder(path, hash);
    assets.push({ path, modified: entry.mtime, bytes: Array.from(bytes) });
  });
  vi.spyOn(book, 'removeAssetPlaceholder').mockImplementation(async (_root, path, hash) => { removePlaceholder(path, hash); });
  const commit = book.commitSync.bind(book);
  vi.spyOn(book, 'commitSync').mockImplementation(async (library, checkpoint, expected, sync, location) => {
    expect(sync?.before).toEqual(metadata());
    await commit(library, checkpoint, expected, sync, location);
    const next = new Map(assets.map(asset => [asset.path, asset]));
    for (const { path } of sync!.deletes) next.delete(path);
    for (const { path, modified, staged } of sync!.writes) {
      const bytes = stages.get(staged.path);
      if (!bytes) throw new Error('暂存附件不存在');
      next.set(path, { path, bytes: Array.from(bytes), modified });
    }
    assets = [...next.values()].sort((a, b) => a.path.localeCompare(b.path));
    placeholders = structuredClone(sync!.placeholders);
  });
  return { book, reads, stages, assets: () => assets, setAssets(next: Asset[]) { assets = structuredClone(next); },
    placeholders: () => placeholders,
  };
}
