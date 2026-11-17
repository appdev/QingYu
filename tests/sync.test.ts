import { beforeEach, describe, expect, it, vi } from 'vitest';
import { Notebook, libraryKey, type Library } from '../src/notebook';
import { SyncFiles, folderPath, metaPath, notePath, assetPath, assetRelative, type Asset, type AssetSync } from '../src/sync-files';
import { syncNotebook } from '../src/sync';
import { createStore, normalizeSettings, openKey } from '../src/sync-store';
import * as core from '../src/vendor/mdsync/core';

vi.hoisted(() => {
  Object.assign(globalThis, { window: globalThis });
  if (typeof navigator !== 'undefined') Object.defineProperty(navigator, 'onLine', { value: true, configurable: true });
});
const encoder = new TextEncoder();
const config = { endpoint: 'https://unit.invalid', bucket: 'test', prefix: 'miaoyan/test', region: 'us-east-1', accessKeyId: 'test-only', secretAccessKey: 'test-only' };
const original: Library = { version: 1, activeId: 'note-a', folders: [{ id: 'default', name: '备忘录' }, { id: 'empty/中文', name: '空文件夹' }],
  notes: [{ id: 'note-a', folderId: 'default', createdAt: 1000, updatedAt: 1000, source: '# 标题\r\n\r\n第一段\r\n\r\n第二段\r\n\r\n第三段🙂\r\n' },
    { id: 'note-b', folderId: 'default', createdAt: 2000, updatedAt: 2000, source: '另一篇笔记' }] };

class Memory {
  values = new Map<string, string>();
  fail = false;
  constructor(library: Library = original) { this.values.set(libraryKey, JSON.stringify(library)); }
  getItem(key: string) { return this.values.get(key) ?? null; }
  setItem(key: string, value: string) { if (this.fail) throw new Error('disk full'); this.values.set(key, value); }
}
const newBook = (library?: Library) => { const memory = new Memory(library); return { memory, book: new Notebook(memory, '') }; };

class S3 {
  objects = new Map<string, { bytes: Uint8Array; etag: string }>();
  sequence = 0;
  failWrites = false;
  failKey = '';
  corruptKey = '';
  omitEtags = false;
  beforePut?: (key: string, headers: Record<string, string>) => Promise<void>;
  handle = async (request: core.Request): Promise<core.Response> => {
    const url = new URL(request.url), key = decodeURIComponent(url.pathname).slice('/test/'.length);
    const headers = Object.fromEntries(Object.entries(request.headers ?? {}).map(([key, value]) => [key.toLowerCase(), value]));
    const respond = (status: number, bytes: Uint8Array = new Uint8Array(), etag?: string): core.Response => ({ status,
      headers: etag && !this.omitEtags ? { etag } : {}, arrayBuffer: bytes.slice().buffer as ArrayBuffer, text: new TextDecoder().decode(bytes), json: {} });
    if (request.method === 'PUT') {
      await this.beforePut?.(key, headers);
      if (this.failWrites || key.endsWith(this.failKey) && this.failKey) return respond(403);
      const found = this.objects.get(key);
      if (headers['if-none-match'] === '*' && found || headers['if-match'] && headers['if-match'] !== found?.etag) return respond(412);
      const bytes = new Uint8Array(request.body ?? new ArrayBuffer(0)).slice();
      const etag = `"opaque-${++this.sequence}"`;
      this.objects.set(key, { bytes, etag }); return respond(200, new Uint8Array(), etag);
    }
    if (request.method === 'GET' && url.searchParams.has('list-type')) {
      const prefix = url.searchParams.get('prefix') ?? '';
      const entries = [...this.objects].filter(([key]) => key.startsWith(prefix)).map(([key, value]) =>
        `<Contents><Key>${key}</Key><ETag>${value.etag}</ETag><LastModified>2026-11-11T00:00:00Z</LastModified></Contents>`).join('');
      return respond(200, encoder.encode(`<ListBucketResult><IsTruncated>false</IsTruncated>${entries}</ListBucketResult>`));
    }
    if (request.method === 'DELETE') { this.objects.delete(key); return respond(204); }
    const found = this.objects.get(key);
    if (!found) return respond(404, encoder.encode('<Error><Code>NoSuchKey</Code></Error>'));
    if (headers['if-none-match'] === found.etag) return respond(304);
    return respond(200, key === this.corruptKey ? encoder.encode('corrupt') : found.bytes, found.etag);
  };
}
let server: S3;
beforeEach(() => { server = new S3(); core.setRequestHandler(server.handle); });
const sync = (book: Notebook) => syncNotebook(book, config);
async function paired() { const a = newBook(), b = newBook(); await sync(a.book); await sync(b.book); return [a, b] as const; }

describe('full library mapping', () => {
  it('preserves names, pin and recoverable trash across mappings and sync', async () => {
    const [a, b] = await paired();
    await a.book.renameNote('note-a', '文件名称'); await a.book.togglePin('note-a'); await a.book.trashNote('note-a');
    expect(new SyncFiles(a.book.data).toLibrary('note-a').notes).toEqual(a.book.data.notes);
    await sync(a.book); await sync(b.book);
    expect(b.book.data.notes.find(note => note.id === 'note-a')).toEqual(a.book.current);
    await b.book.trashNote('note-a', false); await b.book.togglePin('note-a');
    await sync(b.book); await sync(a.book);
    expect(a.book.current.title).toBe('文件名称'); expect(a.book.current.trashed).toBe(false); expect(a.book.current.pinned).toBe(false);
  });
  it('round-trips all folders, stable IDs, dates, source bytes and local selection', () => {
    expect(new SyncFiles(original).toLibrary(original.activeId)).toEqual(original);
    const files = new SyncFiles(original);
    expect([...files.files.keys()].some(path => path.includes('activeId'))).toBe(false);
    expect(files.toLibrary('note-b').activeId).toBe('note-b');
  });
  it('rejects missing metadata, missing bodies and orphaned folder references', () => {
    for (const damage of [(files: SyncFiles) => files.files.delete(metaPath('note-a')), (files: SyncFiles) => files.files.delete(notePath('note-a')),
      (files: SyncFiles) => files.files.delete(folderPath('default'))]) {
      const files = new SyncFiles(original); damage(files); expect(() => files.toLibrary('note-a')).toThrow();
    }
  });
  it('does not accept a remote ID that disagrees with its path', () => {
    const files = new SyncFiles(original); files.put(metaPath('note-a'), JSON.stringify({ id: 'other', folderId: 'default', createdAt: 1000 }));
    expect(() => files.toLibrary('note-a')).toThrow();
  });
});

describe('MDSync application synchronization', () => {
  function assetsBook(initial: Asset[]) {
    const { book } = newBook();
    let assets = structuredClone(initial);
    vi.spyOn(book, 'readSyncAssets').mockImplementation(async () => structuredClone(assets));
    const commit = book.commitSync.bind(book);
    vi.spyOn(book, 'commitSync').mockImplementation(async (library, checkpoint, expected, sync?: AssetSync) => {
      expect(sync?.before).toEqual(assets);
      await commit(library, checkpoint, expected, sync);
      assets = structuredClone(sync!.after);
    });
    return { book, assets: () => assets, setAssets(next: Asset[]) { assets = structuredClone(next); } };
  }
  it('syncs nested binary attachments and hidden asset folders byte-for-byte, without reading them as notes', async () => {
    const files: Asset[] = [{ path: 'images/子目录/图片.webp', bytes: [0, 255, 128, 0, 254], modified: 1 }, { path: '.attachments/document.pdf', bytes: [255, 0, 1], modified: 2 }];
    const a = assetsBook(files), b = assetsBook([]);
    await sync(a.book); await sync(b.book);
    expect(b.assets().sort((a,b) => a.path.localeCompare(b.path))).toEqual(files.sort((a,b) => a.path.localeCompare(b.path)));
    expect(b.book.data.notes).toHaveLength(original.notes.length);
    a.setAssets([{ ...files[0]!, bytes: [3, 0, 4], modified: 3 }]);
    await sync(a.book); await sync(b.book);
    expect(b.assets()).toEqual(a.assets());
  });
  it('preserves both binary conflict versions and converges without duplicate conflict copies', async () => {
    const image: Asset = { path: 'images/a.webp', bytes: [0, 255], modified: 1 };
    const a = assetsBook([image]), b = assetsBook([]);
    await sync(a.book); await sync(b.book);
    a.setAssets([{ ...image, bytes: [7, 255], modified: 2 }]);
    b.setAssets([{ ...image, bytes: [8, 254], modified: 3 }]);
    await sync(b.book); await sync(a.book); await sync(b.book); await sync(a.book);
    expect(a.assets()).toHaveLength(2); expect(b.assets()).toHaveLength(2);
    expect(a.assets().find(a => a.path === image.path)?.bytes).toEqual([7, 255]);
    expect(a.assets().find(a => a.path !== image.path)?.bytes).toEqual([8, 254]);
    expect(b.assets().map(a=>a.bytes).sort()).toEqual(a.assets().map(a=>a.bytes).sort());
  });
  it('preserves a changed attachment when its other side was deleted', async () => {
    const asset: Asset = { path: 'files/a.pdf', bytes: [0, 255], modified: 1 };
    const a = assetsBook([asset]), b = assetsBook([]);
    await sync(a.book); await sync(b.book);
    a.setAssets([]); b.setAssets([{ ...asset, bytes: [9, 254], modified: 2 }]);
    await sync(b.book); await sync(a.book); await sync(b.book);
    expect(a.assets()).toEqual(b.assets()); expect(a.assets()[0]?.bytes).toEqual([9, 254]);
  });
  it('rejects traversal, internal paths and non-canonical asset keys before accessing files', () => {
    for (const path of ['../outside.pdf', '/outside.pdf', '.miaoyan/library.json', '.git/config', 'notes/x.md', 'a/../b.webp', 'a\\b.webp', 'app-config/s3-connection.json']) {
      expect(() => assetRelative(assetPath(path))).toThrow();
    }
    expect(assetRelative(assetPath('子目录/图片.webp'))).toBe('子目录/图片.webp');
  });
  it('accepts attachments larger than the note limit and stops on unreadable note snapshots', async () => {
    const files = new SyncFiles(original);
    files.set(assetPath('files/large.pdf'), new Uint8Array(5 * 1024 * 1024), 1);
    expect(files.files.get(assetPath('files/large.pdf'))?.bytes.length).toBe(5 * 1024 * 1024);
    const { book } = newBook(); book.data.notes[0]!.readOnly = true;
    await expect(sync(book)).rejects.toThrow('不可读'); expect(server.objects.size).toBe(0);
  });
  it('imports unknown notes and empty folders, retaining both existing libraries and local selection', async () => {
    const a = newBook(), other = structuredClone(original);
    other.notes = [{ ...other.notes[0]!, id: 'local-only', source: '本机已有内容' }]; other.activeId = 'local-only';
    const b = newBook(other);
    await sync(a.book); await sync(b.book); await sync(a.book);
    expect(b.book.data.notes).toHaveLength(3);
    expect(b.book.data.activeId).toBe('local-only');
    expect(a.book.data.activeId).toBe('note-a');
    expect(b.book.data.folders).toContainEqual(original.folders[1]);
    expect(b.book.data.notes.find(note => note.id === 'note-a')).toEqual(original.notes[0]);
    expect(new Notebook(b.memory, '').data).toEqual(b.book.data);
  });
  it('merges different notes and non-overlapping edits to the same note', async () => {
    const [a, b] = await paired();
    await a.book.save(a.book.current.source.replace('第一段', 'LOCAL'));
    await b.book.save(b.book.current.source.replace('第三段', 'REMOTE'));
    await sync(b.book); await sync(a.book); await sync(b.book);
    expect(a.book.current.source).toContain('LOCAL'); expect(a.book.current.source).toContain('REMOTE');
    expect(b.book.current.source).toBe(a.book.current.source);
    await a.book.select('note-b'); await a.book.save('第二篇的修改');
    await b.book.save(b.book.current.source + '\n新内容');
    await sync(a.book); await sync(b.book); await sync(a.book);
    expect(a.book.data.notes.find(note => note.id === 'note-b')?.source).toBe('第二篇的修改');
    expect(a.book.data.notes.find(note => note.id === 'note-a')?.source).toContain('新内容');
  });
  it('preserves overlapping edits as a stable conflict note without changing Markdown bytes', async () => {
    const [a, b] = await paired();
    await a.book.save('LOCAL 同段'); await b.book.save('REMOTE 同段');
    await sync(b.book); const result = await sync(a.book); await sync(b.book);
    expect(result.conflicts).toBeGreaterThan(0);
    expect(a.book.data.notes.some(note => note.source === 'LOCAL 同段')).toBe(true);
    expect(a.book.data.notes.some(note => note.source === 'REMOTE 同段' && note.conflictOf === 'note-a')).toBe(true);
    expect(b.book.data.notes).toHaveLength(3);
    await sync(a.book); expect(a.book.data.notes).toHaveLength(3);
  });
  it('keeps legacy note metadata and body together in sync conflict copies', async () => {
    const header = '---\nqingyu-template-app: budget-log\nqingyu-template-data: ';
    const app = (amount: number) => header + JSON.stringify({ expenses: [{ amount, what: '咖啡 ☕' }] }) + '\n---\n# 预算\n\n周末出门。\n';
    const a = newBook(), b = newBook();
    await a.book.save(app(0));
    await b.book.save(app(0));
    await sync(a.book); await sync(b.book);
    await a.book.save(app(18)); await b.book.save(app(22));
    await sync(a.book); await sync(b.book); await sync(a.book);
    for (const { book } of [a, b]) {
      const sources = book.data.notes.filter(note => note.source.startsWith(header)).map(note => note.source);
      expect(sources).toHaveLength(2);
      expect(sources).toContain(app(18));
      expect(sources).toContain(app(22));
    }
  });
  it('detects first-sync same-ID conflicts instead of overwriting either library', async () => {
    const a = newBook(), b = newBook(); await a.book.save('FIRST'); await b.book.save('SECOND');
    await sync(a.book); await sync(b.book);
    expect(b.book.data.notes.some(note => note.source === 'FIRST')).toBe(true);
    expect(b.book.data.notes.some(note => note.source === 'SECOND')).toBe(true);
  });
  it('preserves both folder names during competing renames', async () => {
    const [a, b] = await paired(); a.book.data.folders[0]!.name = 'LOCAL'; b.book.data.folders[0]!.name = 'REMOTE';
    await sync(b.book); await sync(a.book);
    expect(a.book.data.folders.some(folder => folder.name === 'REMOTE')).toBe(true);
    expect(a.book.data.folders.some(folder => folder.name.startsWith('LOCAL'))).toBe(true);
    for (const note of a.book.data.notes) expect(a.book.data.folders.some(folder => folder.id === note.folderId)).toBe(true);
  });
  it('does not commit local content or baseline after a failed network request', async () => {
    const [a] = await paired(); await a.book.save('本机未上传修改'); const before = JSON.stringify(a.book.data);
    server.failKey = core.REMOTE_MANIFEST_KEY;
    await expect(sync(a.book)).rejects.toThrow();
    expect(JSON.stringify(a.book.data)).toBe(before);
    server.failKey = ''; await sync(a.book);
  });
  it('keeps the previous local transaction on quota failure, and can retry after a remote commit', async () => {
    const [a, b] = await paired(); await b.book.save('远端的新内容'); await sync(b.book);
    const before = JSON.stringify(a.book.data), stored = a.memory.getItem(libraryKey);
    a.memory.fail = true; await expect(sync(a.book)).rejects.toThrow('disk full');
    expect(JSON.stringify(a.book.data)).toBe(before); expect(a.memory.getItem(libraryKey)).toBe(stored);
    a.memory.fail = false; await sync(a.book); expect(a.book.current.source).toBe('远端的新内容');
  });
  it('refuses to overwrite local edits made during sync', async () => {
    const [a] = await paired();
    await expect(syncNotebook(a.book, config, message => { if (message === '正在保存同步结果…') a.book.save('同步期间的新输入'); })).rejects.toThrow('发生变化');
    expect(a.book.current.source).toBe('同步期间的新输入');
    await sync(a.book);
  });

  it('flushes late editor input before committing and refuses an unfinished IME composition', async () => {
    const a = newBook();
    await expect(syncNotebook(a.book, config, () => {}, async () => { await a.book.save('尚未进入笔记库的新输入'); return true; })).rejects.toThrow('发生变化');
    expect(a.book.current.source).toBe('尚未进入笔记库的新输入');
    await expect(syncNotebook(a.book, config, () => {}, () => false)).rejects.toThrow('当前输入尚未保存');
    expect(a.book.current.source).toBe('尚未进入笔记库的新输入');
  });
  it('recovers if the cloud commit succeeds but local checkpoint persistence fails', async () => {
    const [a, b] = await paired(); await a.book.save('已提交云端但本机基线尚未落盘');
    const before = JSON.stringify(a.book.data); a.memory.fail = true;
    await expect(sync(a.book)).rejects.toThrow('disk full'); expect(JSON.stringify(a.book.data)).toBe(before);
    await sync(b.book); expect(b.book.current.source).toBe('已提交云端但本机基线尚未落盘');
    a.memory.fail = false; await sync(a.book);
    expect(a.book.data.notes).toHaveLength(2); expect(new Notebook(a.memory, '').data).toEqual(a.book.data);
  });
  it('does not interpret a missing remote index or broken ciphertext as deletions', async () => {
    const [a] = await paired(); const before = JSON.stringify(a.book.data);
    const path = `${config.prefix}/${core.REMOTE_MANIFEST_KEY}`, saved = server.objects.get(path)!;
    server.objects.delete(path); await expect(sync(a.book)).rejects.toThrow('远端索引缺失');
    expect(JSON.stringify(a.book.data)).toBe(before);
    server.objects.set(path, saved); server.corruptKey = path;
    await expect(sync(a.book)).rejects.toThrow(); expect(JSON.stringify(a.book.data)).toBe(before);
  });
  it('requires an existing key for a remote with a manifest', async () => {
    const [a] = await paired(); server.objects.delete(`${config.prefix}/miaoyan-key.json`);
    await expect(sync(a.book)).rejects.toThrow('密钥缺失');
  });
  it('records an initial equal-content baseline so later single-side edits are not false conflicts', async () => {
    const [a, b] = await paired();
    expect(Object.keys(b.book.data.sync!.state.baseline!.files)).toHaveLength(6);
    await b.book.save('单侧修改'); await sync(b.book); await sync(a.book);
    expect(a.book.data.notes).toHaveLength(2); expect(a.book.current.source).toBe('单侧修改');
  });
  it('maps remote deletions and falls back from a deleted local selection', async () => {
    const [a, b] = await paired();
    b.book.data.notes = b.book.data.notes.filter(note => note.id !== 'note-a'); b.book.data.activeId = 'note-b';
    b.book.data.folders = b.book.data.folders.filter(folder => folder.id === 'default');
    await sync(b.book); await sync(a.book);
    expect(a.book.data.notes.map(note => note.id)).toEqual(['note-b']);
    expect(a.book.data.activeId).toBe('note-b'); expect(a.book.data.folders).toHaveLength(1);
  });
  it('preserves two conflicting folder moves of one note', async () => {
    const [a, b] = await paired();
    const folder = await b.book.createFolder('远端文件夹');
    a.book.current.folderId = 'empty/中文'; b.book.current.folderId = folder;
    await sync(b.book); await sync(a.book);
    expect(a.book.data.notes.find(note => note.id === 'note-a')?.folderId).toBe('empty/中文');
    expect(a.book.data.notes.some(note => note.conflictOf === 'note-a' && note.folderId === folder)).toBe(true);
  });
  it('refuses a provider without ETags instead of falling back to unconditional writes', async () => {
    const [a] = await paired(); await a.book.save('不要覆盖'); server.omitEtags = true;
    const before = JSON.stringify(a.book.data);
    await expect(sync(a.book)).rejects.toThrow('ETag'); expect(JSON.stringify(a.book.data)).toBe(before);
  });
});

describe('atomic commit protection', () => {
  it('rejects an obsolete ETag and a duplicate initial create', async () => {
    const store = createStore(normalizeSettings(config)); await openKey(store);
    expect(await store.putManifest!(encoder.encode('first'), null)).toBe(true);
    const etag = server.objects.get(`${config.prefix}/${core.REMOTE_MANIFEST_KEY}`)!.etag;
    expect(await store.putManifest!(encoder.encode('second'), null)).toBe(false);
    expect(await store.putManifest!(encoder.encode('second'), etag)).toBe(true);
    expect(await store.putManifest!(encoder.encode('third'), etag)).toBe(false);
  });
  it('retries two simultaneous additions and converges without losing either note', async () => {
    const [a, b] = await paired();
    await a.book.createNote('default'); const idA = a.book.current.id; await a.book.save('A 新增');
    await b.book.createNote('default'); const idB = b.book.current.id; await b.book.save('B 新增');
    let release!: () => void;
    const both = { promise: new Promise<void>(resolve => { release = resolve; }), resolve: () => release() }; let arrivals = 0;
    server.beforePut = async (key, headers) => {
      if (!key.endsWith(core.REMOTE_MANIFEST_KEY) || arrivals >= 2) return;
      arrivals++; if (arrivals === 2) both.resolve(); await both.promise;
      expect(headers['if-match']).toBeTruthy();
    };
    await Promise.all([sync(a.book), sync(b.book)]);
    server.beforePut = undefined;
    await sync(a.book); await sync(b.book);
    for (const book of [a.book, b.book]) {
      expect(book.data.notes.find(note => note.id === idA)?.source).toBe('A 新增');
      expect(book.data.notes.find(note => note.id === idB)?.source).toBe('B 新增');
    }
  });
  it('bounds repeated races and retains the local transaction on exhaustion', async () => {
    const [a] = await paired(); await a.book.save('等待上传'); const before = JSON.stringify(a.book.data); let races = 0;
    server.beforePut = async key => {
      if (key.endsWith(core.REMOTE_MANIFEST_KEY)) { server.objects.get(key)!.etag = `"race-${++races}"`; }
    };
    await expect(sync(a.book)).rejects.toThrow('持续修改');
    expect(races).toBe(6); expect(JSON.stringify(a.book.data)).toBe(before);
  });
});
