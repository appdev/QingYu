import { readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { describe, expect, it, vi } from 'vitest';
import { Notebook, libraryKey, type Library } from '../src/notebook';
import { syncNotebook } from '../src/sync';
import { createStore, normalizeSettings, type S3Settings } from '../src/sync-store';
import * as core from '../src/vendor/mdsync/core';
import { mockAssetDisk } from './fixtures/sync-assets';
import type { Asset } from '../src/sync-files';

vi.hoisted(() => {
  Object.assign(globalThis, { window: globalThis });
  if (typeof navigator !== 'undefined') Object.defineProperty(navigator, 'onLine', { value: true, configurable: true });
});
const fixture: Library = { version: 1, activeId: 'first', folders: [{ id: 'default', name: '备忘录' }, { id: 'empty', name: '空文件夹' }],
  notes: [{ id: 'first', folderId: 'default', createdAt: 1000, updatedAt: 1000,
    source: '# 云端同步测试\r\n\r\n第一段 中文🙂\r\n\r\n第二段\r\n\r\n第三段\r\n\r\n| 人物 | 身份 |\r\n| --- | --- |\r\n| 婴宁 | 狐仙 |\r\n\r\n```mermaid\r\nflowchart LR\r\nA --> B\r\n```\r\n\r\n$E=mc^2$\r\n' },
    { id: 'second', folderId: 'default', createdAt: 2000, updatedAt: 2000, source: '独立笔记' }] };
function device(library = fixture) {
  let value = JSON.stringify(library), fail = false;
  const storage = { getItem: (key: string) => key === libraryKey ? value : null,
    setItem: (_key: string, next: string) => { if (fail) throw new Error('test quota'); value = next; } };
  const book = new Notebook(storage, '');
  return { ...mockAssetDisk(book), storage, setFail: (next: boolean) => { fail = next; } };
}

describe.skipIf(!process.env.MIAOYAN_S3_CONFIG)('real S3 integration (explicit private config only)', () => {
  it('validates atomic writes, full libraries, conflicts, concurrent writers, durable retries and native transport preparation', async () => {
    const settings = JSON.parse(await readFile(process.env.MIAOYAN_S3_CONFIG!, 'utf8')) as S3Settings;
    const config = normalizeSettings(settings), root = config.prefix;
    const calls = { count: 0, rejectedWrites: 0 };
    let arrivals = 0, release!: () => void, barrier: Promise<void> | undefined;
    core.setRequestHandler(async request => {
      const url = new URL(request.url);
      if (url.origin !== new URL(config.endpoint).origin) throw new Error('Test request escaped its configured origin');
      if (barrier && request.method === 'PUT' && url.pathname.endsWith(core.REMOTE_MANIFEST_KEY) && arrivals < 2) {
        arrivals++; if (arrivals === 2) release(); await barrier;
      }
      const response = await fetch(request.url, { method: request.method, headers: request.headers, body: request.body,
        redirect: 'error', signal: AbortSignal.timeout(30000) });
      calls.count++;
      if (response.status === 412 || response.status === 409) calls.rejectedWrites++;
      const arrayBuffer = await response.arrayBuffer();
      return { status: response.status, headers: Object.fromEntries(response.headers), arrayBuffer, text: new TextDecoder().decode(arrayBuffer), json: {} };
    });
    const targets = [config, { ...config, prefix: `${root}-cas` }];
    const report: Record<string, unknown> = { prefix: root, cases: [], calls };
    const done = (name: string) => (report.cases as string[]).push(name);
    try {
      const cas = createStore(targets[1]!);
      expect(await cas.putManifest!(new TextEncoder().encode('initial'), null)).toBe(true);
      const read = await cas.getIfChanged!(core.REMOTE_MANIFEST_KEY, null);
      if (read.status !== 'found' || !read.etag) throw new Error('S3 did not return an ETag');
      expect(await cas.putManifest!(new TextEncoder().encode('next'), read.etag)).toBe(true);
      expect(await cas.putManifest!(new TextEncoder().encode('stale'), read.etag)).toBe(false);
      expect(await cas.putManifest!(new TextEncoder().encode('duplicate'), null)).toBe(false);
      done('real ETag CAS and create-only writes');

      const a = device(), initialB = structuredClone(fixture);
      initialB.notes = [{ ...initialB.notes[0]!, id: 'b-existing', source: 'B 的已有笔记' }]; initialB.activeId = 'b-existing';
      const b = device(initialB), sync = (book: Notebook) => syncNotebook(book, config);
      await sync(a.book); await sync(b.book); await sync(a.book);
      expect(a.book.data.notes).toHaveLength(3); expect(b.book.data.notes).toHaveLength(3);
      expect(b.book.data.activeId).toBe('b-existing'); expect(a.book.data.activeId).toBe('first');
      expect(b.book.data.notes.find(note => note.id === 'first')).toEqual(fixture.notes[0]);
      expect(b.book.data.folders).toContainEqual(fixture.folders[1]);
      expect(new Notebook(b.storage, '').data).toEqual(b.book.data);
      done('initial libraries, full IDs/dates/CRLF/Markdown, empty folder and local selection');

      await b.book.select('first'); await a.book.save(a.book.current.source.replace('第一段', '本机第一段'));
      await b.book.save(b.book.current.source.replace('第三段', '远端第三段'));
      await sync(b.book); await sync(a.book); await sync(b.book);
      expect(a.book.current.source).toContain('本机第一段'); expect(a.book.current.source).toContain('远端第三段');
      expect(b.book.current.source).toBe(a.book.current.source);
      done('three-way non-overlapping merge');

      await a.book.save('LOCAL OVERLAP'); await b.book.save('REMOTE OVERLAP');
      await sync(b.book); await sync(a.book); await sync(b.book); await sync(a.book);
      expect(a.book.data.notes).toHaveLength(4); expect(b.book.data.notes).toHaveLength(4);
      expect(a.book.data.notes.some(note => note.source === 'REMOTE OVERLAP' && note.conflictOf === 'first')).toBe(true);
      expect(a.book.data.notes.some(note => note.source === 'LOCAL OVERLAP')).toBe(true);
      done('overlapping conflict retains both, idempotent next sync');

      await a.book.createNote('default'); const idA = a.book.current.id; await a.book.save('Concurrent A');
      await b.book.createNote('default'); const idB = b.book.current.id; await b.book.save('Concurrent B');
      const beforeRejects = calls.rejectedWrites;
      barrier = new Promise<void>(resolve => { release = resolve; });
      await Promise.all([sync(a.book), sync(b.book)]); barrier = undefined;
      expect(calls.rejectedWrites).toBeGreaterThan(beforeRejects);
      await sync(a.book); await sync(b.book);
      for (const book of [a.book, b.book]) {
        expect(book.data.notes.find(note => note.id === idA)?.source).toBe('Concurrent A');
        expect(book.data.notes.find(note => note.id === idB)?.source).toBe('Concurrent B');
      }
      done('simultaneous real S3 pushes: rejected stale writer retries and both additions survive');

      const before = JSON.stringify(a.book.data); a.setFail(true);
      await expect(sync(a.book)).rejects.toThrow('test quota'); expect(JSON.stringify(a.book.data)).toBe(before);
      a.setFail(false); await sync(a.book); expect(new Notebook(a.storage, '').data).toEqual(a.book.data);
      done('atomic local checkpoint and quota recovery');

      const attachment: Asset = { path: '.attachments/子目录/image.webp', bytes: [0, 255, 128, 254, 0], modified: 1000 };
      a.setAssets([attachment]);
      await sync(a.book); await sync(b.book);
      expect(b.assets()).toEqual([attachment]);
      a.setAssets([{ ...attachment, bytes: [7, 255, 128], modified: 2000 }]);
      b.setAssets([{ ...attachment, bytes: [8, 254, 129], modified: 3000 }]);
      await sync(b.book); await sync(a.book); await sync(b.book); await sync(a.book);
      expect(a.assets()).toHaveLength(2); expect(b.assets()).toHaveLength(2);
      expect(a.assets().map(asset => asset.bytes).sort()).toEqual(b.assets().map(asset => asset.bytes).sort());
      a.setAssets([]); await sync(a.book); await sync(b.book); expect(b.assets()).toEqual([]);
      done('nested binary attachments, byte-exact roundtrip, conflict copies, idempotence and deletion');

      const native = { ...config, prefix: `${root}-native` }, store = createStore(native);
      await store.put('native-probe', new TextEncoder().encode('native-before'));
      const current = await store.getIfChanged!('native-probe', null);
      if (current.status !== 'found' || !current.etag) throw new Error('Native probe ETag missing');
      const sign = core.createS3Signer(native), nativeCases = [];
      for (const [method, headers, data, expected] of [
        ['GET', {}, undefined, 200],
        ['PUT', { 'If-Match': current.etag }, 'native-after', 200],
        ['PUT', { 'If-Match': current.etag }, 'must-not-overwrite', 412],
        ['PUT', { 'If-None-Match': '*' }, 'must-not-create', 412],
      ] as const) {
        const body = data === undefined ? undefined : new TextEncoder().encode(data);
        const signed = await sign({ method, key: `${native.prefix}/native-probe`, headers: headers as Record<string, string>, body });
        nativeCases.push({ request: { ...signed, method, body: body ? [...body] : null }, expected });
      }
      await writeFile(path.join(path.dirname(process.env.MIAOYAN_S3_CONFIG!), 'native-probe.json'),
        JSON.stringify({ endpoint: native.endpoint, bucket: native.bucket, prefix: native.prefix, cases: nativeCases }), { mode: 0o600 });
      done('prepared signed cases for the actual Rust HTTP transport');
      report.success = true;
    } finally {
      barrier = undefined; release?.();
      const cleanup: number[] = [];
      for (const target of targets) {
        const store = createStore(target); const keys = await store.list('');
        for (const key of keys) await store.delete(key);
        cleanup.push((await store.list('')).length);
      }
      report.cleanupRemainingObjects = cleanup;
      await writeFile(process.env.MIAOYAN_S3_REPORT ?? 'artifacts/mdsync-spike/live-results.json', JSON.stringify(report, null, 2) + '\n');
      expect(cleanup).toEqual([0, 0]);
    }
  }, 180000);
});
