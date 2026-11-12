import type { Folder, Library, Note } from './notebook';
import { validLibrary } from './notebook';
import type { Adapter, Scope } from './vendor/mdsync/core';

const encoder = new TextEncoder(), decoder = new TextDecoder('utf-8', { fatal: true });
export const encodeId = (id: string) => btoa(String.fromCharCode(...encoder.encode(id))).replaceAll('+', '-').replaceAll('/', '_').replace(/=+$/, '');
export const notePath = (id: string) => `notes/${encodeId(id)}.md`;
export const metaPath = (id: string) => `library/notes/${encodeId(id)}.json`;
export const folderPath = (id: string) => `library/folders/${encodeId(id)}.json`;
export type Asset = { path: string; bytes: number[]; modified: number };
export type AssetSync = { before: Asset[]; after: Asset[] };
export const MAX_ASSET_BYTES = 64 * 1024 * 1024;
export const MAX_ASSETS_TOTAL = 256 * 1024 * 1024;
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
    ...(note.title !== undefined ? { title: note.title } : {}), ...(note.pinned !== undefined ? { pinned: note.pinned } : {}), ...(note.trashed !== undefined ? { trashed: note.trashed } : {}), ...(note.filename ? { filename: note.filename } : {}) };
}

export class SyncFiles implements Adapter {
  readonly files = new Map<string, { bytes: Uint8Array; mtime: number }>();
  private dirs = new Set(['']);
  constructor(library: Library, assets: Asset[] = []) {
    for (const folder of library.folders) this.put(folderPath(folder.id), JSON.stringify(folder), 1);
    for (const note of library.notes) {
      this.put(metaPath(note.id), JSON.stringify(noteMeta(note)), note.createdAt);
      this.put(notePath(note.id), note.source, note.updatedAt);
    }
    for (const asset of assets) this.set(assetPath(asset.path), new Uint8Array(asset.bytes), asset.modified);
  }
  put(path: string, text: string, mtime = Date.now()) { this.set(path, encoder.encode(text), mtime); }
  set(path: string, bytes: Uint8Array, mtime = Date.now()) {
    if (!ownedPath(path)) throw new Error('同步文件路径无效');
    const attachment = path.startsWith('assets/');
    if (bytes.length > (attachment ? MAX_ASSET_BYTES : 4 * 1024 * 1024) || !Number.isFinite(mtime) || mtime < 0) throw new Error('同步文件超出限制');
    if (attachment && bytes.length + [...this.files].reduce((total, [key, value]) => total + (key.startsWith('assets/') && key !== path ? value.bytes.length : 0), 0) > MAX_ASSETS_TOTAL) throw new Error('附件快照超过 256 MiB，已停止同步');
    this.files.set(path, { bytes: bytes.slice(), mtime });
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
    if (value) return { type: 'file' as const, ctime: value.mtime, mtime: value.mtime, size: value.bytes.length };
    return this.dirs.has(path) ? { type: 'folder' as const, ctime: 0, mtime: 0, size: 0 } : null;
  }
  async exists(path: string) { return this.files.has(path) || this.dirs.has(path); }
  async readBinary(path: string) {
    const value = this.files.get(path);
    if (!value) throw new Error('同步文件缺失');
    return value.bytes.slice().buffer as ArrayBuffer;
  }
  async read(path: string) { return decoder.decode(await this.readBinary(path)); }
  async writeBinary(path: string, bytes: ArrayBuffer) { this.set(path, new Uint8Array(bytes)); }
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
    this.set(to, old.bytes, old.mtime); this.files.delete(from);
  }
  toLibrary(activeId: string): Library {
    const folders: Folder[] = [], notes: Note[] = [], consumed = new Set<string>();
    for (const [path, value] of this.files) {
      if (path.startsWith('assets/')) { assetRelative(path); consumed.add(path); }
      else if (path.startsWith('library/folders/')) {
        const folder = JSON.parse(decoder.decode(value.bytes)) as Folder;
        if (!folder || typeof folder.id !== 'string' || path !== folderPath(folder.id)) throw new Error('远端文件夹元数据无效');
        folders.push(folder); consumed.add(path);
      } else if (path.startsWith('library/notes/')) {
        const meta = JSON.parse(decoder.decode(value.bytes)) as NoteMeta;
        if (!meta || typeof meta.id !== 'string' || path !== metaPath(meta.id)) throw new Error('远端笔记元数据无效');
        const bodyPath = notePath(meta.id), body = this.files.get(bodyPath);
        if (!body) throw new Error('远端笔记正文缺失，未替换本机笔记');
        notes.push({ ...noteMeta(meta), source: decoder.decode(body.bytes), updatedAt: body.mtime });
        consumed.add(path); consumed.add(bodyPath);
      }
    }
    if (consumed.size !== this.files.size) throw new Error('远端存在无法映射的笔记，未替换本机笔记');
    const library: Library = { version: 1, folders, notes, activeId: notes.some(note => note.id === activeId) ? activeId : notes[0]?.id ?? '' };
    if (!validLibrary(library)) throw new Error('同步后的笔记库格式无效，未替换本机笔记');
    return library;
  }
  toAssets(): Asset[] {
    const assets = [...this.files].filter(([path]) => path.startsWith('assets/')).map(([path, value]) => ({ path: assetRelative(path), bytes: Array.from(value.bytes), modified: value.mtime }));
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
    return assets;
  }
}
