import type { Folder, Library, Note } from './notebook';
import { validLibrary } from './notebook';
import type { Adapter, Scope } from './vendor/mdsync/core';

const encoder = new TextEncoder(), decoder = new TextDecoder('utf-8', { fatal: true });
export const encodeId = (id: string) => btoa(String.fromCharCode(...encoder.encode(id))).replaceAll('+', '-').replaceAll('/', '_').replace(/=+$/, '');
export const notePath = (id: string) => `notes/${encodeId(id)}.md`;
export const metaPath = (id: string) => `library/notes/${encodeId(id)}.json`;
export const folderPath = (id: string) => `library/folders/${encodeId(id)}.json`;
export type Asset = { path: string; bytes: number[]; modified: number };
export type AssetStat = Omit<Asset, 'bytes'> & { size: number };
export type StagedAsset = { path: string; hash: string };
export type AssetPlaceholder = { hash: string; size: number; mtime: number };
export type AssetPlaceholders = Record<string, AssetPlaceholder>;
export type AssetSync = { before: AssetStat[]; writes: { path: string; staged: StagedAsset; modified: number; previousHash?: string }[]; deletes: { path: string; hash: string }[]; placeholders: AssetPlaceholders };
type SyncFile = { size: number; mtime: number } & ({ bytes: Uint8Array } | { original: AssetStat } | { staged: StagedAsset } | { placeholder: string });
type AssetIO = {
  read(asset: AssetStat): Promise<Uint8Array>;
  stage(bytes: Uint8Array): Promise<StagedAsset>;
  readStage(asset: StagedAsset): Promise<Uint8Array>;
};
export const MAX_ASSET_BYTES = 64 * 1024 * 1024;
export function assetPath(path: string) { return `assets/${encodeId(path)}.bin`; }
export function assetRelative(path: string) {
  const match = /^assets\/([A-Za-z0-9_-]+)\.bin$/.exec(path);
  if (!match) throw new Error('附件同步路径无效');
  const value = decoder.decode(Uint8Array.from(atob(match[1]!.replaceAll('-', '+').replaceAll('_', '/')), char => char.charCodeAt(0)));
  if (assetPath(value) !== path || value.split('/').at(-1)?.toLowerCase() === 's3-connection.json' || value.includes('\\') || value.includes(':') || /[\x00-\x1f\x7f]/.test(value) || (/\.(md|markdown|txt)$/i.test(value) && !value.split('/').includes('.attachments')) ||
      value.split('/').some(part => !part || part === '.' || part === '..' || ['.miaoyan', '.miaoyan-conflicts', '.git'].includes(part.toLowerCase()))) throw new Error('附件路径超出笔记库范围或属于内部文件');
  return value;
}
export function ownedPath(path: string) {
  if (/^(notes\/[A-Za-z0-9_-]+\.md|library\/(notes|folders)\/[A-Za-z0-9_-]+\.json)$/.test(path)) return true;
  try { assetRelative(path); return true; } catch { return false; }
}
export const scope: Scope = { includes: ownedPath, owns: ownedPath, canDescend: path => ['', 'notes', 'library', 'library/notes', 'library/folders', 'assets'].includes(path),
  classify: () => 'vault', isIgnoredByPattern: () => false, getCategory: () => null, isSettingsJson: path => path.endsWith('.json'),
  loadsOnStartup: () => false, pluginIdOf: () => null };
export type NoteMeta = Omit<Note, 'source' | 'updatedAt'>;
export function noteMeta(note: NoteMeta): NoteMeta {
  return { id: note.id, folderId: note.folderId, createdAt: note.createdAt, ...(note.conflictOf ? { conflictOf: note.conflictOf } : {}),
    ...(note.title !== undefined ? { title: note.title } : {}), ...(note.pinned !== undefined ? { pinned: note.pinned } : {}), ...(note.trashed !== undefined ? { trashed: note.trashed } : {}), ...(note.filename ? { filename: note.filename } : {}),
    ...(note.share ? { share: note.share } : {}), ...(note.followTitle !== undefined ? { followTitle: note.followTitle } : {}) };
}

export class SyncFiles implements Adapter {
  readonly files = new Map<string, SyncFile>();
  private dirs = new Set(['']);
  constructor(library: Library, private assets: AssetStat[] = [], private assetIO?: AssetIO) {
    for (const folder of library.folders) this.put(folderPath(folder.id), JSON.stringify(folder), 1);
    for (const note of library.notes) {
      const names = library.sync?.localNoteNames;
      const alias = names && Object.hasOwn(names, note.id) ? names[note.id] : undefined;
      const meta = alias && alias.localTitle === note.title && alias.localFilename === note.filename
        ? { ...note, title: alias.title ?? undefined, filename: alias.filename ?? undefined } : note;
      this.put(metaPath(note.id), JSON.stringify(noteMeta(meta)), note.createdAt);
      this.put(notePath(note.id), note.source, note.updatedAt);
    }
    for (const asset of assets) this.add(assetPath(asset.path), { original: asset, size: asset.size, mtime: asset.modified });
  }
  put(path: string, text: string, mtime = Date.now()) { this.set(path, encoder.encode(text), mtime); }
  private set(path: string, bytes: Uint8Array, mtime: number) {
    if (!ownedPath(path) || path.startsWith('assets/') || bytes.length > 4 * 1024 * 1024 || !Number.isFinite(mtime) || mtime < 0) throw new Error('同步文件无效或超出限制');
    this.add(path, { bytes: bytes.slice(), size: bytes.length, mtime });
  }
  async writeAsset(path: string, bytes: Uint8Array, mtime = Date.now()) {
    assetRelative(path);
    if (!this.assetIO || bytes.length > MAX_ASSET_BYTES || !Number.isFinite(mtime) || mtime < 0) throw new Error('附件写入不可用或超出限制');
    const staged = await this.assetIO.stage(bytes);
    this.add(path, { staged, size: bytes.length, mtime });
  }
  setPlaceholder(path: string, { hash, size, mtime }: AssetPlaceholder) {
    assetRelative(path);
    this.add(path, { placeholder: hash, size, mtime });
  }
  private add(path: string, value: SyncFile) {
    this.files.set(path, value);
    const pieces = path.split('/'); pieces.pop();
    while (pieces.length) { this.dirs.add(pieces.join('/')); pieces.pop(); }
  }
  async list(path: string) {
    const prefix = path ? `${path}/` : '';
    return { files: [...this.files.keys()].filter(key => key.startsWith(prefix) && !key.slice(prefix.length).includes('/')),
      folders: [...this.dirs].filter(key => key.startsWith(prefix) && key !== path && key && !key.slice(prefix.length).includes('/')) };
  }
  async stat(path: string) {
    const value = this.files.get(path);
    if (value) return { type: 'file' as const, ctime: value.mtime, mtime: value.mtime, size: value.size };
    return this.dirs.has(path) ? { type: 'folder' as const, ctime: 0, mtime: 0, size: 0 } : null;
  }
  verifiedAsset(path: string) {
    const value = this.files.get(path);
    if (value && 'staged' in value) return { hash: value.staged.hash, size: value.size, mtime: value.mtime };
    if (value && 'placeholder' in value) return { hash: value.placeholder, size: value.size, mtime: value.mtime };
  }
  async exists(path: string) { return this.files.has(path) || this.dirs.has(path); }
  async readBinary(path: string) {
    const value = this.files.get(path);
    if (!value) throw new Error('同步文件缺失');
    if ('bytes' in value) return value.bytes.slice().buffer as ArrayBuffer;
    if ('placeholder' in value) throw new Error('附件尚未下载');
    if (!this.assetIO) throw new Error('附件读取不可用');
    const bytes = 'original' in value ? await this.assetIO.read(value.original) : await this.assetIO.readStage(value.staged);
    if (bytes.length !== value.size) throw new Error('同步期间本地附件发生变化');
    return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer;
  }
  async read(path: string) { return decoder.decode(await this.readBinary(path)); }
  async writeBinary(path: string, bytes: ArrayBuffer) {
    if (path.startsWith('assets/')) await this.writeAsset(path, new Uint8Array(bytes));
    else this.set(path, new Uint8Array(bytes), Date.now());
  }
  async write(path: string, text: string) { this.put(path, text); }
  async mkdir(path: string) { if (!scope.canDescend(path)) throw new Error('同步目录无效'); this.dirs.add(path); }
  async remove(path: string) { if (!this.files.delete(path)) throw new Error('同步文件缺失'); }
  async rmdir(path: string) { if ([...this.files.keys()].some(key => key.startsWith(`${path}/`))) throw new Error('同步目录非空'); this.dirs.delete(path); }
  async trashSystem() { return false; }
  async trashLocal(path: string) { await this.remove(path); }
  async rename(from: string, to: string) {
    if (this.files.has(to)) throw new Error('同步文件已存在');
    const old = this.files.get(from);
    if (!old) throw new Error('同步文件缺失');
    if (!ownedPath(to)) throw new Error('同步文件路径无效');
    this.add(to, old); this.files.delete(from);
  }
  toLibrary(activeId: string): Library {
    const folders: Folder[] = [], notes: Note[] = [], consumed = new Set<string>();
    for (const [path, value] of this.files) {
      if (path.startsWith('assets/')) { assetRelative(path); consumed.add(path); }
      else if ('bytes' in value && path.startsWith('library/folders/')) {
        const folder = JSON.parse(decoder.decode(value.bytes)) as Folder;
        if (!folder || typeof folder.id !== 'string' || path !== folderPath(folder.id)) throw new Error('远端文件夹元数据无效');
        folders.push(folder); consumed.add(path);
      } else if ('bytes' in value && path.startsWith('library/notes/')) {
        const meta = JSON.parse(decoder.decode(value.bytes)) as NoteMeta;
        if (!meta || typeof meta.id !== 'string' || path !== metaPath(meta.id)) throw new Error('远端笔记元数据无效');
        const bodyPath = notePath(meta.id), body = this.files.get(bodyPath);
        if (!body || !('bytes' in body)) throw new Error('远端笔记正文缺失，未替换本机笔记');
        notes.push({ ...noteMeta(meta), source: decoder.decode(body.bytes), updatedAt: body.mtime });
        consumed.add(path); consumed.add(bodyPath);
      }
    }
    if (consumed.size !== this.files.size) throw new Error('远端存在无法映射的笔记，未替换本机笔记');
    const library: Library = { version: 1, folders, notes, activeId: notes.some(note => note.id === activeId) ? activeId : notes[0]?.id ?? '' };
    if (!validLibrary(library)) throw new Error('同步后的笔记库格式无效，未替换本机笔记');
    return library;
  }
  async assetChanges(originalEntries: Record<string, { hash: string }>): Promise<{ sync: AssetSync; count: number }> {
    const assets = [...this.files].filter(([path]) => path.startsWith('assets/')).map(([path, value]) => ({ path: assetRelative(path), value }));
    const names = new Set<string>();
    for (const asset of assets) {
      const name = asset.path.toLowerCase();
      if (names.has(name)) throw new Error('附件名称仅大小写不同，无法跨平台同步');
      names.add(name);
    }
    for (const asset of assets) {
      const parts = asset.path.toLowerCase().split('/'); parts.pop();
      while (parts.length) {
        if (names.has(parts.join('/'))) throw new Error('附件文件与目录同名，已停止同步');
        parts.pop();
      }
    }
    const before = new Set(this.assets.map(asset => asset.path));
    const finalPaths = new Set(assets.filter(({ value }) => !('placeholder' in value)).map(asset => asset.path));
    const deletes = this.assets.filter(asset => !finalPaths.has(asset.path)).map(({ path }) => ({ path, hash: originalEntries[assetPath(path)]!.hash }));
    const writes: AssetSync['writes'] = [];
    const placeholders: AssetPlaceholders = Object.create(null);
    for (const { path, value } of assets) {
      if ('placeholder' in value) {
        placeholders[path] = { hash: value.placeholder, size: value.size, mtime: value.mtime };
        continue;
      }
      if ('original' in value && value.original.path === path) continue;
      const previousHash = before.has(path) ? originalEntries[assetPath(path)]!.hash : undefined;
      if (!this.assetIO || 'bytes' in value) throw new Error('附件暂存不可用');
      const staged = 'staged' in value ? value.staged : await this.assetIO.stage(await this.assetIO.read(value.original));
      writes.push({ path, staged, modified: value.mtime, previousHash });
    }
    return { sync: { before: this.assets, writes, deletes, placeholders }, count: assets.length };
  }
}
