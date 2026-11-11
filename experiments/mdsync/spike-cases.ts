// Copied by run.mjs into the pinned checkout so upstream helpers use the same Vitest runtime.
import { Notebook, libraryKey } from '__MIAOYAN_NOTEBOOK__';
import { FakeStorage } from '@tests/helpers/fake-storage';
import { TestSession, pairedSessions, useEncryptionKey } from '@tests/helpers/session';
import { describe, expect, it, vi } from 'vitest';
import { REMOTE_MANIFEST_KEY } from '@/core/constants';
import { createS3Adapter, defaultS3Config } from '@/features/storage/backends/s3';
import type { StorageAdapter } from '@/features/storage/types';
import { autoMergeOp } from '@/features/sync/apply/auto-merge';
import { freeConflictCopyPath } from '@/features/sync/apply/conflict-copy';
import { loadRemoteText } from '@/features/sync/changes/content';
import { readHistory } from '@/features/sync/history/remote';
import { pullPathsOp } from '@/features/sync/operations/pull';
import { pushPathsOp } from '@/features/sync/operations/push';
import { keepBothConflictOp } from '@/features/sync/operations/resolve';
import { fetchRemoteManifest } from '@/features/sync/remote/manifest';
import { startS3 } from './s3-loopback.fixture';

// Prototype transport replacement only. Node fetch does not exercise Tauri/CORS/TLS certificate handling.
vi.mock('obsidian', async (original) => ({
  ...(await original<Record<string, unknown>>()),
  requestUrl: async (params: { url: string; method: string; headers: Record<string, string>; body?: ArrayBuffer }) => {
    if (new URL(params.url).hostname !== '127.0.0.1') throw new Error('Only loopback test endpoints are allowed');
    const response = await fetch(params.url, { method: params.method, headers: params.headers, body: params.body });
    const arrayBuffer = await response.arrayBuffer();
    return { status: response.status, headers: Object.fromEntries(response.headers), arrayBuffer,
      text: new TextDecoder().decode(arrayBuffer), json: {} };
  },
}));

useEncryptionKey();
const BASE = '# 同步测试\n\n第一段：中文与 emoji 🙂\n\n第二段：保留原文\n\n第三段：原始内容\n';
const FORMATS = BASE + '\n- [x] 任务\n\n| 列一 | 列二 |\n| --- | --- |\n| 中文 | **粗体** |\n\n'
  + '> 引用\n\n```kotlin\nval greeting = "你好"\n```\n\n```mermaid\nflowchart LR\n  A[开始] --> B[结束]\n```\n\n$E=mc^2$\n';
const PATH = 'notes/稳定标识.md';

async function push(device: TestSession) {
  const result = await device.compare();
  await pushPathsOp(device.deps(), result, result.diff.localChanges.map(c => c.path), device.context());
}

async function pull(device: TestSession) {
  const result = await device.compare();
  await pullPathsOp(device.deps(), result, result.diff.remoteChanges.map(c => c.path), device.context());
}

async function diverged(local: string, remote: string) {
  const [a, b] = pairedSessions();
  a.adapter.putText(PATH, BASE);
  await push(a);
  await pull(b);
  b.adapter.putText(PATH, remote);
  await push(b);
  a.adapter.putText(PATH, local);
  return [a, b] as const;
}

describe('MiaoYan / MDSync isolated integration', () => {
  it('round-trips a real Notebook Markdown body without touching its folder or selection', async () => {
    const values = new Map<string, string>();
    const local = { getItem: (k: string) => values.get(k) ?? null, setItem: (k: string, v: string) => { values.set(k, v); } };
    const book = new Notebook(local, FORMATS.replaceAll('\n', '\r\n'));
    book.save(book.current.source);
    const before = structuredClone(book.data);
    const [a, b] = pairedSessions();
    const remotePath = `notes/${book.current.id}.md`;
    a.adapter.putText(remotePath, book.current.source);
    await push(a);
    await pull(b);
    expect(b.text(remotePath)).toBe(book.current.source);
    b.adapter.putText(remotePath, book.current.source + '\r\n手机补充\r\n');
    await push(b);
    await pull(a);
    book.save(a.text(remotePath));
    expect(book.current.source).toContain('手机补充');
    expect(book.data.activeId).toBe(before.activeId);
    expect(book.data.folders).toEqual(before.folders);
    expect(new Notebook(local, '').current.source).toBe(a.text(remotePath));
    expect(values.has(libraryKey)).toBe(true);
  });

  it('merges separate paragraphs and converges on both devices', async () => {
    const [a, b] = await diverged(BASE.replace('第一段', '本机修改第一段'), BASE.replace('第三段', '远端修改第三段'));
    const outcome = await autoMergeOp(a.deps(), await a.compare(), a.context());
    expect([...outcome.touchedPaths]).toEqual([PATH]);
    expect(a.text(PATH)).toContain('本机修改第一段');
    expect(a.text(PATH)).toContain('远端修改第三段');
    await push(a);
    await pull(b);
    expect(b.text(PATH)).toBe(a.text(PATH));
    expect((await a.compare()).diff.conflicts).toEqual([]);
  });

  it('leaves overlapping edits untouched, then keeps and synchronizes both versions', async () => {
    const local = BASE.replace('第二段', '本机的第二段'), remote = BASE.replace('第二段', '远端的第二段');
    const [a, b] = await diverged(local, remote);
    const result = await a.compare();
    const outcome = await autoMergeOp(a.deps(), result, a.context());
    expect(outcome.touchedPaths.size).toBe(0);
    expect(a.text(PATH)).toBe(local);
    await keepBothConflictOp(a.deps(), await a.compare(), PATH, a.context());
    const copied = (await a.compare()).diff.localChanges.find(c => c.path.includes('(conflict from'));
    expect(copied).toBeDefined();
    expect(a.text(copied!.path)).toBe(remote);
    await push(a);
    await pull(b);
    expect(b.text(PATH)).toBe(local);
    expect(b.text(copied!.path)).toBe(remote);
  });

  it('does not auto-merge first-sync collisions with no common baseline', async () => {
    const [a, b] = pairedSessions();
    b.adapter.putText(PATH, '远端已有内容');
    await push(b);
    a.adapter.putText(PATH, '本机已有内容');
    const result = await a.compare();
    expect(result.diff.conflicts[0]?.baselineHash).toBeNull();
    expect((await autoMergeOp(a.deps(), result, a.context())).touchedPaths.size).toBe(0);
    expect(a.text(PATH)).toBe('本机已有内容');
    expect(b.text(PATH)).toBe('远端已有内容');
  });

  it('finds a fresh conflict-copy path without overwriting a previous copy', async () => {
    const [a] = pairedSessions();
    const first = await freeConflictCopyPath(a.adapter.asDataAdapter(), PATH, 'Phone');
    a.adapter.putText(first, '需要保留的旧副本');
    const second = await freeConflictCopyPath(a.adapter.asDataAdapter(), PATH, 'Phone');
    expect(second).not.toBe(first);
    expect(a.text(first)).toBe('需要保留的旧副本');
  });

  it('preserves typing that lands while an auto-merge downloads remote content', async () => {
    const [a] = await diverged(BASE.replace('第一段', 'LOCAL'), BASE.replace('第三段', 'REMOTE'));
    const result = await a.compare();
    const get = a.storage.get.bind(a.storage);
    const typed = a.text(PATH) + '正在输入的新内容';
    const spy = vi.spyOn(a.storage, 'get').mockImplementation(async key => {
      a.adapter.putText(PATH, typed);
      return get(key);
    });
    try {
      const outcome = await autoMergeOp(a.deps(), result, a.context());
      expect(outcome.touchedPaths.size).toBe(0);
      expect(a.text(PATH)).toBe(typed);
    } finally { spy.mockRestore(); }
  });

  it('keeps local edits and the old baseline on an upload failure, then retries', async () => {
    const [a, b] = pairedSessions();
    a.adapter.putText(PATH, BASE);
    await push(a);
    await pull(b);
    const baseline = structuredClone(a.state.baseline);
    a.adapter.putText(PATH, FORMATS);
    const spy = vi.spyOn(a.storage, 'put').mockRejectedValueOnce(new Error('simulated offline'));
    try { await expect(push(a)).rejects.toThrow('simulated offline'); }
    finally { spy.mockRestore(); }
    expect(a.state.baseline).toEqual(baseline);
    expect(a.text(PATH)).toBe(FORMATS);
    await push(a);
    await pull(b);
    expect(b.text(PATH)).toBe(FORMATS);
  });

  it('retains a readable previous version with history enabled', async () => {
    const a = new TestSession();
    const original = a.deps.bind(a);
    a.deps = () => ({ ...original(), history: { horizon: null } });
    a.adapter.putText(PATH, BASE);
    await push(a);
    const oldHash = a.state.baseline!.files[PATH]!.hash;
    a.adapter.putText(PATH, FORMATS);
    await push(a);
    const deps = a.deps();
    const history = await readHistory(deps.storage, deps.key, '');
    expect(history.unreadable).toEqual([]);
    expect(history.view.pushes.size).toBe(2);
    expect(await loadRemoteText(deps, oldHash)).toBe(BASE);
  });

  it('round-trips through the actual S3 adapter over loopback HTTP and refuses revoked credentials', async () => {
    const server = await startS3(0);
    const config = { ...defaultS3Config(), endpoint: server.url, bucket: server.bucket,
      region: 'us-east-1', prefix: 'miaoyan-test/', accessKeyId: 'test-only', secretAccessKey: 'test-only-secret' };
    const make = (id: string, storage: StorageAdapter) => {
      const session = new TestSession(id), original = session.deps.bind(session);
      session.deps = () => ({ ...original(), storage });
      return session;
    };
    try {
      const storeA = createS3Adapter(config), storeB = createS3Adapter(config);
      const a = make('desktop', storeA), b = make('phone-simulated', storeB);
      a.adapter.putText(PATH, FORMATS);
      await push(a);
      await pull(b);
      expect(b.text(PATH)).toBe(FORMATS);
      expect(server.keys().every(key => key.startsWith(config.prefix))).toBe(true);
      const bytes = await storeA.get(REMOTE_MANIFEST_KEY);
      expect(bytes).not.toBeNull();
      expect(new TextDecoder().decode(bytes!)).not.toContain('同步测试');
      expect(await storeA.putIfAbsent('create-only', new Uint8Array([1]))).toBe(true);
      expect(await storeB.putIfAbsent('create-only', new Uint8Array([2]))).toBe(false);
      server.revoke(config.accessKeyId);
      await expect(storeB.get(REMOTE_MANIFEST_KEY)).rejects.toThrow();
    } finally { server.stop(); }
  });

  it('KNOWN LIMITATION: both concurrent pushes can report success while one new note leaves HEAD', async () => {
    const storage = new FakeStorage();
    const a = new TestSession('A', storage), b = new TestSession('B', storage);
    a.adapter.putText(PATH, BASE);
    await push(a);
    await pull(b);
    a.adapter.putText('only-A.md', 'A 的新增笔记');
    b.adapter.putText('only-B.md', 'B 的新增笔记');
    const compared = await Promise.all([a.compare(), b.compare()]);
    const bothChecked = Promise.withResolvers<void>(), firstDone = Promise.withResolvers<void>();
    const put = storage.put.bind(storage);
    let writes = 0;
    storage.put = async (key, bytes) => {
      if (key === REMOTE_MANIFEST_KEY) {
        writes++;
        if (writes === 1) await bothChecked.promise;
        else if (writes === 2) { bothChecked.resolve(); await firstDone.promise; }
      }
      await put(key, bytes);
    };
    const jobs = [a, b].map((device, i) => pushPathsOp(device.deps(), compared[i]!,
      compared[i]!.diff.localChanges.map(c => c.path), device.context()));
    try {
      await Promise.race(jobs);
      firstDone.resolve();
      const settled = await Promise.allSettled(jobs);
      expect(settled.map(job => job.status)).toEqual(['fulfilled', 'fulfilled']);
      const head = await fetchRemoteManifest(storage, a.deps().key, '');
      const added = ['only-A.md', 'only-B.md'].filter(path => head?.files[path]);
      expect(added).toHaveLength(1);
      expect(head?.files[PATH]).toBeDefined();
    } finally {
      bothChecked.resolve();
      firstDone.resolve();
      await Promise.allSettled(jobs);
      storage.put = put;
    }
  });
});
