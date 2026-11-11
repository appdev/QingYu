import { noteText } from './document-text';
import type { State } from './vendor/mdsync/core';
import { invoke } from '@tauri-apps/api/core';
import type { Asset, AssetSync } from './sync-files';

export const libraryKey = 'miaoyan-editor.notebook.v1';
export const legacyDraftKey = 'miaoyan-editor.draft.v1';
export type Folder = { id: string; name: string; path?: string };
export type Note = { id: string; folderId: string; source: string; createdAt: number; updatedAt: number; conflictOf?: string; title?: string; pinned?: boolean; trashed?: boolean; path?: string; filename?: string; readOnly?: boolean };
export const noteTitle = (note: Pick<Note, 'source' | 'title'>) => note.title ?? noteText(note.source).title;
export type Checkpoint = { version: 1; target: string; state: State };
export type Library = { version: 1; folders: Folder[]; notes: Note[]; activeId: string; sync?: Checkpoint };
export type EditorBuffer = { noteId: string; source: string };
type Storage = Pick<globalThis.Storage, 'getItem' | 'setItem'>;

function initialLibrary(source: string): Library {
  const now = Date.now(), id = crypto.randomUUID();
  return { version: 1, folders: [{ id: 'default', name: '备忘录' }], notes: [{ id, folderId: 'default', source, createdAt: now, updatedAt: now }], activeId: id };
}

export function validLibrary(value: unknown): value is Library {
  if (!value || typeof value !== 'object') return false;
  const data = value as Library;
  if (data.version !== 1 || !Array.isArray(data.folders) || !data.folders.length || !Array.isArray(data.notes)) return false;
  const folders = new Set<string>(), notes = new Set<string>();
  for (const folder of data.folders) {
    if (!folder || typeof folder.id !== 'string' || !folder.id || folder.id.length > 500 || folders.has(folder.id) || typeof folder.name !== 'string' || !folder.name.trim()) return false;
    folders.add(folder.id);
  }
  for (const note of data.notes) {
    if (!note || typeof note.id !== 'string' || !note.id || note.id.length > 500 || notes.has(note.id) || !folders.has(note.folderId) || typeof note.source !== 'string' ||
        !Number.isFinite(note.createdAt) || !Number.isFinite(note.updatedAt) || !Number.isFinite(new Date(note.createdAt).getTime()) || !Number.isFinite(new Date(note.updatedAt).getTime()) ||
        (note.conflictOf !== undefined && (typeof note.conflictOf !== 'string' || !note.conflictOf)) ||
        (note.title !== undefined && (typeof note.title !== 'string' || !note.title.trim() || note.title.length > 500 || /[\r\n\0]/.test(note.title))) ||
        (note.pinned !== undefined && typeof note.pinned !== 'boolean') || (note.trashed !== undefined && typeof note.trashed !== 'boolean')) return false;
    notes.add(note.id);
  }
  if (data.sync !== undefined) {
    const sync = data.sync, state = sync?.state, base = state?.baseline;
    if (!sync || sync.version !== 1 || typeof sync.target !== 'string' || !sync.target || !state || typeof state.deviceId !== 'string' ||
        !state.deviceId || (state.vaultId !== null && typeof state.vaultId !== 'string') || !state.hashCache || typeof state.hashCache !== 'object' ||
        (base !== null && (!base || base.version !== 3 || typeof base.snapshotId !== 'string' || typeof base.vaultId !== 'string' || !base.files || typeof base.files !== 'object' ||
          Object.values(base.files).some(entry => !entry || !/^[a-f0-9]{64}$/.test(entry.hash) || !Number.isFinite(entry.size) || entry.size < 0 || !Number.isFinite(entry.mtime))))) return false;
  }
  return !notes.size ? data.activeId === '' : notes.has(data.activeId);
}

export class Notebook {
  data: Library;
  loadError: boolean;
  private persisted = false;
  private pending: Promise<unknown> = Promise.resolve();
  private disk?: { root: string; revision: number };
  private externalRevision = 0;
  external = false;
  constructor(private storage: Storage, starter: string) {
    this.data = initialLibrary(starter);
    this.loadError = false;
    try {
      const saved = storage.getItem(libraryKey);
      if (saved !== null) {
        const parsed: unknown = JSON.parse(saved);
        if (!validLibrary(parsed)) throw new Error('笔记库格式无效');
        this.data = parsed;
        this.persisted = true;
      } else this.data = initialLibrary(storage.getItem(legacyDraftKey) ?? starter);
    } catch { this.loadError = true; }
  }
  get current() { return this.data.notes.find(note => note.id === this.data.activeId)!; }
  get root() { return this.disk?.root; }
  async attachExternal() {
    this.adopt(await invoke('workspace_snapshot')); this.external = true;
  }
  activateExternal(path: string) { return this.run(async () => {
    if (!this.external || !this.disk) throw new Error('尚未打开外部文件目录');
    this.adopt(await invoke('workspace_activate_path', { path, revision: this.disk.revision }));
    this.externalRevision++;
  }); }
  async openFiles() {
    const seed = this.loadError ? null : { ...this.data, notes: this.data.notes.map(note => ({ ...note, title: note.title ?? (note.source.trim() ? noteText(note.source).title : undefined) })) };
    this.adopt(await invoke('library_open', { seed })); this.loadError = false;
  }
  private adopt(snapshot: { library: Library; root: string; revision: number }) {
    if (!validLibrary(snapshot.library)) throw new Error('本地文件笔记库格式无效');
    this.data = snapshot.library; this.disk = { root: snapshot.root, revision: snapshot.revision }; this.persisted = true; this.loadError = false;
  }
  private run<T>(action: () => Promise<T>): Promise<T> {
    const operation = this.pending.then(action); this.pending = operation.catch(() => undefined); return operation;
  }
  private async commit(next: Library, intent = 'edit', assets?: AssetSync) {
    if (this.loadError) throw new Error('笔记库读取失败，未覆盖原有数据');
    if (this.disk) this.adopt(await invoke(this.external ? 'workspace_commit' : 'library_commit', { next, intent, revision: this.disk.revision, ...(assets ? { assets } : {}) }));
    else { this.storage.setItem(libraryKey, JSON.stringify(next)); this.data = next; this.persisted = true; }
  }
  save(source: string, id = this.data.activeId) { const externalRevision = this.externalRevision; return this.run(async () => {
    if (externalRevision !== this.externalRevision) throw new Error('文件已在外部变化，旧的延迟保存已取消');
    const current = this.data.notes.find(note => note.id === id);
    if (!current) { if (!id) return; throw new Error('笔记不存在'); }
    const changed = current.source !== source;
    if (changed && current.readOnly) throw new Error('文件内容损坏或不可读，已阻止覆盖');
    if (!changed && this.persisted) return;
    await this.commit(changed ? { ...this.data, notes: this.data.notes.map(note => note.id === current.id ? { ...note, source, updatedAt: Date.now() } : note) } : this.data);
  }); }
  select(id: string) { return this.run(async () => {
    if (!this.data.notes.some(note => note.id === id)) throw new Error('笔记不存在');
    if (this.external && this.disk) {
      const snapshot = await invoke<{ library: Library; root: string; revision: number } | null>('workspace_select', { noteId: id, revision: this.disk.revision });
      if (!snapshot) return false;
      this.adopt(snapshot); return true;
    }
    await this.commit({ ...this.data, activeId: id });
    return true;
  }); }
  createNote(folderId: string, title = '') { return this.run(async () => {
    if (!this.data.folders.some(folder => folder.id === folderId)) throw new Error('文件夹不存在');
    const now = Date.now(), id = crypto.randomUUID();
    await this.commit({ ...this.data, notes: [{ id, folderId, source: '', createdAt: now, updatedAt: now, ...(title ? { title } : {}) }, ...this.data.notes], activeId: id }, 'create');
  }); }
  createFolder(name: string, parentId = 'default') { return this.run(async () => {
    if (this.external) throw new Error('临时文件模式不管理文件夹，请返回笔记库操作');
    name = name.trim();
    if (!name || name.startsWith('.') || /[\\/:\x00-\x1f]/.test(name)) throw new Error('请输入有效的文件夹名称');
    const parent = this.data.folders.find(folder => folder.id === parentId);
    if (!parent) throw new Error('父文件夹不存在');
    if (this.disk) {
      const before = new Set(this.data.folders.map(folder => folder.id));
      this.adopt(await invoke('library_folder_action', { action: 'create', folderId: parentId, value: name, revision: this.disk.revision }));
      return this.data.folders.find(folder => !before.has(folder.id))!.id;
    }
    const path = [parent.path ?? (parent.id === 'default' ? '' : parent.name), name].filter(Boolean).join('/');
    if (this.data.folders.some(folder => (folder.path ?? folder.name).toLocaleLowerCase() === path.toLocaleLowerCase())) throw new Error('已有同名文件夹');
    const folder = { id: crypto.randomUUID(), name, path };
    await this.commit({ ...this.data, folders: [...this.data.folders, folder] });
    return folder.id;
  }); }
  folderAction(action: 'rename' | 'move' | 'trash' | 'reveal' | 'terminal', folderId: string | null, value?: string) { return this.run(async () => {
    if (this.external) throw new Error('临时文件模式不管理文件夹，请返回笔记库操作');
    if (this.disk) {
      this.adopt(await invoke('library_folder_action', { action, folderId, value, revision: this.disk.revision })); return;
    }
    if (action !== 'rename') throw new Error('请在桌面应用中操作本地文件夹');
    const folder = this.data.folders.find(folder => folder.id === folderId);
    if (!folder || folder.id === 'default' || folder.path === '') throw new Error('不能修改笔记库根文件夹');
    const name = value?.trim();
    if (!name || name.startsWith('.') || /[\\/:\x00-\x1f]/.test(name)) throw new Error('文件夹名称无效');
    const from = folder.path ?? folder.name, to = [...from.split('/').slice(0, -1), name].join('/');
    if (this.data.folders.some(other => other.id !== folderId && (other.path ?? other.name).toLocaleLowerCase() === to.toLocaleLowerCase())) throw new Error('已有同名文件夹');
    await this.commit({ ...this.data, folders: this.data.folders.map(other => other.id === folderId ? { ...other, name, path: to } : other.path?.startsWith(from + '/') ? { ...other, path: to + other.path.slice(from.length) } : other) });
  }); }
  private async updateNote(id: string, patch: Partial<Pick<Note, 'title' | 'pinned' | 'trashed' | 'folderId'>>) {
    if (this.external && ('trashed' in patch || 'folderId' in patch)) throw new Error('临时文件模式不移动或删除文件，请在文件管理器中操作');
    if (!this.data.notes.some(note => note.id === id)) throw new Error('笔记不存在');
    await this.commit({ ...this.data, notes: this.data.notes.map(note => note.id === id ? { ...note, ...patch } : note) });
  }
  renameNote(id: string, title: string) { return this.run(async () => {
    title = title.trim();
    if (!title || title.length > 500 || /[\r\n\0]/.test(title)) throw new Error('文件名称无效，不能包含换行或为空');
    await this.updateNote(id, { title });
  }); }
  togglePin(id: string) { return this.run(async () => {
    const note = this.data.notes.find(note => note.id === id);
    if (!note) throw new Error('笔记不存在');
    await this.updateNote(id, { pinned: !note.pinned });
  }); }
  moveNote(id: string, folderId: string) { return this.run(async () => {
    if (!this.data.folders.some(folder => folder.id === folderId)) throw new Error('文件夹不存在');
    await this.updateNote(id, { folderId, trashed: false });
  }); }
  trashNote(id: string, trashed = true) { return this.run(() => this.updateNote(id, { trashed })); }
  duplicateNote(id: string) { return this.run(async () => {
    const note = this.data.notes.find(note => note.id === id);
    if (!note) throw new Error('笔记不存在');
    if (note.readOnly) throw new Error('文件不可读，不能创建空白副本');
    // MiaoYan getDupeName uses " Copy", then " Copy 2", without changing the body.
    const base = (note.filename ? note.filename.replace(/\.[^.]+$/, '') : noteTitle(note)).replace(/ Copy(?: \d+)?$/, '').slice(0, 145);
    let title = `${base} Copy`, number = 2;
    while (this.data.notes.some(other => other.folderId === note.folderId && noteTitle(other) === title)) title = `${base} Copy ${number++}`;
    const now = Date.now(), copy: Note = { id: crypto.randomUUID(), folderId: note.folderId, title, source: note.source, createdAt: now, updatedAt: now };
    await this.commit({ ...this.data, notes: [copy, ...this.data.notes], activeId: copy.id }, this.disk ? `duplicate:${id}` : 'duplicate');
  }); }
  async readSyncAssets(): Promise<Asset[]> {
    if (this.external) throw new Error('外部文件不在 S3 同步范围内');
    if (!this.disk) return [];
    return invoke('library_sync_assets', { root: this.disk.root, revision: this.disk.revision });
  }
  commitSync(library: Library, checkpoint: Checkpoint, expected: string, assets?: AssetSync) { return this.run(async () => {
    if (this.external) throw new Error('外部文件不在 S3 同步范围内');
    if (JSON.stringify(this.data) !== expected) throw new Error('同步期间本机笔记发生变化，已保留新内容，请再次同步');
    const next = { ...library, sync: checkpoint };
    if (!validLibrary(next)) throw new Error('同步结果格式无效，未覆盖本机笔记');
    await this.commit(next, 'sync', assets);
  }); }
  importFiles(directory = false) { return this.run(async () => { this.adopt(await invoke('library_import', { directory })); }); }
  chooseRoot() { return this.run(async () => { this.adopt(await invoke('library_choose')); }); }
  reload() {
    if (this.external && this.disk) return this.reconcile(this.disk.root).then(() => {});
    return this.run(async () => { if (this.disk) this.adopt(await invoke('library_reload')); });
  }
  reconcile(root: string, buffer?: EditorBuffer) { return this.run(async () => {
    if (!this.disk || this.disk.root !== root) return { changed: false, backups: [] as string[] };
    const result = await invoke<{ snapshot: { library: Library; root: string; revision: number } | null; backups: string[] }>(this.external ? 'workspace_reconcile' : 'library_reconcile', { root, revision: this.disk.revision, buffer });
    if (result.snapshot) { this.adopt(result.snapshot); this.externalRevision++; }
    return { changed: Boolean(result.snapshot), backups: result.backups };
  }); }
}

export function visibleNotes(library: Library, folderId: string | null, query: string, trashed = false) {
  const search = query.trim().toLocaleLowerCase();
  const selected = library.folders.find(folder => folder.id === folderId);
  const path = selected?.path ?? selected?.name;
  const included = new Set(library.folders.filter(folder => folder.id === folderId || path && (folder.path ?? folder.name).startsWith(path + '/')).map(folder => folder.id));
  return library.notes.filter(note => Boolean(note.trashed) === trashed && (!folderId || included.has(note.folderId)) && (!search || note.source.toLocaleLowerCase().includes(search) || noteTitle(note).toLocaleLowerCase().includes(search)))
    .sort((a, b) => Number(Boolean(b.pinned && !b.trashed)) - Number(Boolean(a.pinned && !a.trashed)) || b.updatedAt - a.updatedAt);
}

export function groupNotes(notes: Note[]) {
  const currentYear = new Date().getFullYear();
  const groups: { year: number; month: number; label: string; notes: Note[] }[] = [];
  for (const note of [...notes].sort((a, b) => Number(Boolean(b.pinned && !b.trashed)) - Number(Boolean(a.pinned && !a.trashed)) || b.updatedAt - a.updatedAt)) {
    const date = new Date(note.updatedAt), year = note.pinned && !note.trashed ? 0 : date.getFullYear();
    const month = year === currentYear ? date.getMonth() + 1 : 0;
    const last = groups.at(-1);
    if (last?.year === year && last.month === month) last.notes.push(note);
    else groups.push({ year, month, label: !year ? '置顶' : month ? ['一月', '二月', '三月', '四月', '五月', '六月', '七月', '八月', '九月', '十月', '十一月', '十二月'][month - 1] : `${year}年`, notes: [note] });
  }
  return groups;
}
