import { noteText, noteTitle } from './document-text';
import { dailyFolderName, dailyNoteId, dailyTitle, findDailyNote, type DailyNoteSettings } from './daily-note';
import type { State } from './vendor/mdsync/core';
import { invoke } from '@tauri-apps/api/core';
import type { AssetPlaceholders, AssetStat, AssetSync, StagedAsset } from './sync-files';
import { ServerLibrary, type ServerSnapshot } from './server-library';

export const libraryKey = 'miaoyan-editor.notebook.v1';
export const legacyDraftKey = 'miaoyan-editor.draft.v1';
export type Folder = { id: string; name: string; path?: string };
// 分享状态只记在元数据里；没有 share 等同于未分享。
export type NoteShare = { status: 'shared' | 'unshared'; url?: string };
export type Note = { id: string; folderId: string; source: string; createdAt: number; updatedAt: number; conflictOf?: string; title?: string; pinned?: boolean; trashed?: boolean; path?: string; filename?: string; readOnly?: boolean; share?: NoteShare };

/** 还没写过一个字的新笔记：正文完全为空、不在废纸篓、可以编辑。对应 Pinto 的 ListCompose.isOff。 */
export function isBlankNote(note: Note | undefined, source = note?.source ?? '') {
  return Boolean(note) && !note!.trashed && !note!.readOnly && source === '';
}
// 和 Pinto 一样，废纸篓里的笔记不显示分享标记。
export const isShared = (note: Pick<Note, 'share' | 'trashed'>) => note.share?.status === 'shared' && !note.trashed;
export function validShare(share: unknown): share is NoteShare {
  if (!share || typeof share !== 'object') return false;
  const { status, url } = share as NoteShare;
  if (status !== 'shared' && status !== 'unshared') return false;
  if (url === undefined) return true;
  if (typeof url !== 'string' || url.length > 2048) return false;
  try { return ['http:', 'https:'].includes(new URL(url).protocol); } catch { return false; }
}
export { noteTitle } from './document-text';
export type Checkpoint = { target: string; state: State } & ({ version: 1 } | {
  version: 2; placeholderCount: number; placeholderDigest?: string;
});
export type Library = { version: 1; folders: Folder[]; notes: Note[]; activeId: string; sync?: Checkpoint };
export type EditorBuffer = { noteId: string; source: string };
export type NoteVersion = { id: string; updatedAt: number };
type FileSnapshot = { library: Library; root: string; revision: number };
type Storage = Pick<globalThis.Storage, 'getItem' | 'setItem'>;

function encodeAsset(bytes: Uint8Array) {
  const encoded: string[] = [];
  const chunkSize = 3 * 8192;
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    encoded.push(btoa(String.fromCharCode(...bytes.subarray(offset, offset + chunkSize))));
  }
  return encoded.join('');
}

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
        (note.pinned !== undefined && typeof note.pinned !== 'boolean') || (note.trashed !== undefined && typeof note.trashed !== 'boolean') ||
        (note.share !== undefined && !validShare(note.share))) return false;
    notes.add(note.id);
  }
  if (data.sync !== undefined) {
    const sync = data.sync, state = sync?.state, base = state?.baseline;
    if (!sync || (sync.version !== 1 && (sync.version !== 2 || !Number.isInteger(sync.placeholderCount) || sync.placeholderCount < 1 || sync.placeholderCount > 30_000)) ||
        typeof sync.target !== 'string' || !sync.target || !state || typeof state.deviceId !== 'string' ||
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
  noteWindow = false;
  private remote?: ServerLibrary;
  get server() { return Boolean(this.remote); }
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
  get assetRevision() { return this.disk?.revision ?? 0; }
  get hasNoteHistory() { return Boolean(this.disk && !this.external && !this.loadError); }
  noteVersions(noteId: string) {
    return invoke<NoteVersion[]>('library_note_versions', { noteId });
  }
  noteVersion(noteId: string, version: string) {
    return invoke<string>('library_note_version', { noteId, version });
  }
  restoreNoteVersion(noteId: string, version: string, expectedSource: string, root: string | undefined) { return this.run(async () => {
    if (!this.hasNoteHistory || !root || root !== this.root || this.data.activeId !== noteId) throw new Error('原笔记已切换，请重新打开版本历史');
    this.adopt(await invoke<FileSnapshot>('library_restore_note_version', { root, noteId, version, expectedSource }));
    this.externalRevision++;
  }); }
  importAttachment(id: string, upload: { name: string; bytes: number[] }): Promise<{ name: string; path: string; image: boolean }> { return this.run(async () => {
    const note = this.data.notes.find(note => note.id === id);
    if (!note || note.readOnly || note.trashed) throw new Error('此笔记不可添加附件');
    if (this.remote) return this.remote.importAttachment(id, upload);
    if (!this.disk) throw new Error('请先打开本地笔记库');
    return invoke('attachment_import', { id, upload });
  }); }
  async openServer() { this.remote = new ServerLibrary(); this.adoptServer(await this.remote.load()); }
  private adoptServer(snapshot: ServerSnapshot, activeId = this.data.activeId) {
    const notes = snapshot.notes;
    const library: Library = { version: 1, folders: snapshot.folders, notes,
      activeId: notes.some(note => note.id === activeId) ? activeId : (notes.find(note => !note.trashed) ?? notes[0])?.id ?? '' };
    if (!validLibrary(library)) throw new Error('服务器笔记库格式无效');
    this.data = library; this.remote!.accept(snapshot); this.persisted = true; this.loadError = false;
  }
  stageDraft(source: string) { if (this.current) this.remote?.stageDraft(this.current.id, source, this.current.source); }
  refreshServer(ready: () => boolean) { return this.run(async () => {
    if (!this.remote || !ready()) return false;
    const snapshot = await this.remote.load();
    if (!ready()) return false;
    const previous = JSON.stringify(this.data); this.adoptServer(snapshot);
    return previous !== JSON.stringify(this.data);
  }); }
  async attachExternal() {
    this.adopt(await invoke('workspace_snapshot')); this.external = true;
  }
  async attachNoteWindow() {
    const snapshot = await invoke<FileSnapshot>('workspace_note_snapshot');
    this.adopt(snapshot, snapshot.library.activeId);
    this.noteWindow = true;
  }
  activateExternal(path: string) { return this.run(async () => {
    if (!this.external || !this.disk) throw new Error('尚未打开外部文件目录');
    this.adopt(await invoke('workspace_activate_path', { path, revision: this.disk.revision }));
    this.externalRevision++;
  }); }
  async openFiles() {
    const seed = this.loadError ? null : { ...this.data, notes: this.data.notes.map(note => ({ ...note, title: note.title ?? (note.source.trim() ? noteText(note.source).title : undefined) })) };
    try { this.adopt(await invoke('library_open', { seed })); }
    catch (error) { this.loadError = true; throw error; }
  }
  private adopt(snapshot: FileSnapshot, activeId = this.noteWindow ? this.data.activeId : snapshot.library.activeId) {
    if (!validLibrary(snapshot.library)) throw new Error('本地文件笔记库格式无效');
    this.data = activeId === snapshot.library.activeId ? snapshot.library
      : { ...snapshot.library, activeId: snapshot.library.notes.some(note => note.id === activeId) ? activeId : '' };
    this.disk = { root: snapshot.root, revision: snapshot.revision }; this.persisted = true; this.loadError = false;
  }
  private run<T>(action: () => Promise<T>): Promise<T> {
    const operation = this.pending.then(action); this.pending = operation.catch(() => undefined); return operation;
  }
  private async commit(next: Library, intent = 'edit', assets?: AssetSync) {
    if (this.loadError) throw new Error('笔记库读取失败，未覆盖原有数据');
    if (this.remote) {
      const snapshot = await this.remote.commit(this.data, next, intent);
      if (snapshot) this.adoptServer(snapshot, next.activeId); else this.data = next;
    }
    else if (this.disk) this.adopt(await invoke(this.external ? 'workspace_commit' : 'library_commit', { next, intent, revision: this.disk.revision, ...(assets ? { assets } : {}) }), this.noteWindow ? next.activeId : undefined);
    else { this.storage.setItem(libraryKey, JSON.stringify(next)); this.data = next; this.persisted = true; }
  }
  save(source: string, id = this.data.activeId, expectedSource?: string) { const externalRevision = this.externalRevision, root = this.disk?.root; return this.run(async () => {
    if (externalRevision !== this.externalRevision) throw new Error('文件已在外部变化，旧的延迟保存已取消');
    if (root !== this.disk?.root) throw new Error('笔记库位置已更改，旧的延迟保存已取消');
    const current = this.data.notes.find(note => note.id === id);
    if (!current) { if (!id) return; throw new Error('笔记不存在'); }
    if (expectedSource !== undefined && (current.source !== expectedSource || current.trashed)) {
      throw new Error('笔记正文已更新，请重新查看引用');
    }
    if (expectedSource !== undefined && !this.disk && !this.remote && this.persisted
      && this.storage.getItem(libraryKey) !== JSON.stringify(this.data)) {
      throw new Error('笔记库已在其他窗口更新，请刷新后重试');
    }
    const changed = current.source !== source;
    if (changed && current.readOnly) throw new Error('文件内容损坏或不可读，已阻止覆盖');
    if (!changed && this.persisted) return;
    if (root && !this.external && !this.remote && changed) {
      if (this.loadError) throw new Error('笔记库读取失败，未覆盖原有数据');
      this.adopt(await invoke('library_save_note', { root, noteId: id, expectedSource: current.source, source }));
      return;
    }
    await this.commit(changed ? { ...this.data, notes: this.data.notes.map(note => note.id === current.id ? { ...note, source, updatedAt: Date.now() } : note) } : this.data);
  }); }
  select(id: string) { return this.run(async () => {
    if (!this.data.notes.some(note => note.id === id)) throw new Error('笔记不存在');
    if (this.noteWindow) {
      this.adopt(await invoke('workspace_note_snapshot', { noteId: id }), id);
      return true;
    }
    if (this.external && this.disk) {
      const snapshot = await invoke<{ library: Library; root: string; revision: number } | null>('workspace_select', { noteId: id, revision: this.disk.revision });
      if (!snapshot) return false;
      this.adopt(snapshot); return true;
    }
    await this.commit({ ...this.data, activeId: id });
    return true;
  }); }
  private async insertNote(folderId: string, title: string, source: string, id: string = crypto.randomUUID(), now = Date.now()) {
    if (!this.data.folders.some(folder => folder.id === folderId)) throw new Error('文件夹不存在');
    await this.commit({ ...this.data, notes: [{ id, folderId, source, createdAt: now, updatedAt: now, ...(title ? { title } : {}) }, ...this.data.notes], activeId: id }, 'create');
  }
  createNote(folderId: string, title = '') { return this.run(() => this.insertNote(folderId, title, '')); }
  openToday(settings: DailyNoteSettings, now = new Date()) { return this.run(async () => {
    if (this.external) throw new Error('请返回笔记库打开今日笔记');
    for (let attempt = 0; ; attempt++) {
      const { folder, note } = findDailyNote(this.data, settings, now);
      if (note) {
        if (this.current?.id !== note.id) await this.commit({ ...this.data, activeId: note.id });
        return false;
      }
      try {
        const folderId = folder?.id ?? await this.insertFolder(dailyFolderName, 'default');
        const title = dailyTitle(now, settings.todayTitle);
        let generation = 0, id = await dailyNoteId(folderId, now, generation);
        while (this.data.notes.some(note => note.id === id)) id = await dailyNoteId(folderId, now, ++generation);
        await this.insertNote(folderId, title, `# ${title}\n`, id, now.getTime());
        return true;
      } catch (error) {
        // Another browser can win each of the two writes: first the folder, then the note.
        if (!this.remote || attempt >= 2) throw error;
        const snapshot = await this.remote.load().catch(() => undefined);
        if (!snapshot) throw error;
        const current = findDailyNote({ ...this.data, folders: snapshot.folders, notes: snapshot.notes }, settings, now);
        if (!current.note && (folder || !current.folder)) throw error;
        this.adoptServer(snapshot);
      }
    }
  }); }

  createFolder(name: string, parentId = 'default') { return this.run(() => this.insertFolder(name, parentId)); }
  private async insertFolder(name: string, parentId: string) {
    if (this.external) throw new Error('临时文件模式不管理文件夹，请返回笔记库操作');
    name = name.trim();
    if (!name || name.startsWith('.') || /[\\/:\x00-\x1f]/.test(name)) throw new Error('请输入有效的文件夹名称');
    const parent = this.data.folders.find(folder => folder.id === parentId);
    if (!parent) throw new Error('父文件夹不存在');
    if (this.remote) {
      const before = new Set(this.data.folders.map(folder => folder.id));
      this.adoptServer(await this.remote.createFolder(parentId, name));
      return this.data.folders.find(folder => !before.has(folder.id))!.id;
    }
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
  }
  folderAction(action: 'rename' | 'move' | 'trash' | 'reveal' | 'terminal', folderId: string | null, value?: string) { return this.run(async () => {
    if (this.remote) {
      if (!folderId || !value || !['rename', 'move'].includes(action)) throw new Error('请在服务器配置笔记目录；此操作不适用于 Web');
      this.adoptServer(await this.remote.folder(action as 'rename' | 'move', folderId, value)); return;
    }
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
  private async updateNote(id: string, patch: Partial<Pick<Note, 'title' | 'pinned' | 'trashed' | 'folderId' | 'share'>>) {
    if (this.external && ('trashed' in patch || 'folderId' in patch)) throw new Error('临时文件模式不移动或删除文件，请在文件管理器中操作');
    if (!this.data.notes.some(note => note.id === id)) throw new Error('笔记不存在');
    await this.commit({ ...this.data, notes: this.data.notes.map(note => note.id === id ? { ...note, ...patch } : note) });
  }
  renameNote(id: string, title: string) { return this.run(async () => {
    title = title.trim();
    if (!title || title.length > 500 || /[\r\n\0]/.test(title)) throw new Error('文件名称无效，不能包含换行或为空');
    await this.updateNote(id, { title });
  }); }
  setShare(id: string, share: NoteShare) { return this.run(async () => {
    if (!validShare(share)) throw new Error('分享链接无效');
    await this.updateNote(id, { share });
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
    await this.commit({ ...this.data, notes: [copy, ...this.data.notes], activeId: copy.id }, this.disk || this.remote ? `duplicate:${id}` : 'duplicate');
  }); }
  async readSyncAssets(): Promise<AssetStat[]> {
    if (this.external) throw new Error('外部文件不在 S3 同步范围内');
    if (this.remote) throw new Error('Web 使用服务器笔记库，S3 同步请在桌面端操作');
    if (!this.disk) return [];
    return invoke('library_sync_assets', { root: this.disk.root, revision: this.disk.revision });
  }
  async readAssetPlaceholders(): Promise<AssetPlaceholders> {
    if (!this.disk) return {};
    return invoke('library_asset_placeholders', { root: this.disk.root, revision: this.disk.revision });
  }
  async readAsset(asset: AssetStat, location: { root: string | undefined; revision: number }): Promise<Uint8Array> {
    const bytes = await invoke<ArrayBuffer | number[]>('library_read_asset', { ...location, asset });
    return new Uint8Array(bytes);
  }
  stageAsset(batch: string, bytes: Uint8Array, location: { root: string | undefined; revision: number }): Promise<StagedAsset> {
    return invoke('library_stage_asset', { ...location, batch, encoded: encodeAsset(bytes) });
  }
  async readStagedAsset(batch: string, asset: StagedAsset): Promise<Uint8Array> {
    const bytes = await invoke<ArrayBuffer | number[]>('library_read_staged_asset', { batch, asset });
    return new Uint8Array(bytes);
  }
  releaseAssetStage(batch: string): Promise<void> {
    return invoke('library_release_asset_stage', { batch });
  }
  materializeAsset(root: string, path: string, hash: string, bytes: Uint8Array) { return this.run(async () => {
    if (!this.disk || this.external || this.disk.root !== root) throw new Error('笔记库已切换，请重新下载附件');
    this.adopt(await invoke<FileSnapshot>('library_materialize_asset', {
      root, revision: this.disk.revision, path, hash, encoded: encodeAsset(bytes),
    }));
  }); }
  removeAssetPlaceholder(root: string, path: string, hash: string) { return this.run(async () => {
    if (!this.disk || this.external || this.disk.root !== root) throw new Error('笔记库已切换');
    this.adopt(await invoke<FileSnapshot>('library_delete_asset_placeholder', { root, revision: this.disk.revision, path, hash }));
  }); }
  commitSync(library: Library, checkpoint: Checkpoint, expected: string, assets?: AssetSync,
    location = { root: this.root, revision: this.assetRevision }) { return this.run(async () => {
    if (this.external) throw new Error('外部文件不在 S3 同步范围内');
    if (this.remote) throw new Error('Web 使用服务器笔记库，S3 同步请在桌面端操作');
    if (this.root !== location.root || this.assetRevision !== location.revision || JSON.stringify(this.data) !== expected) throw new Error('同步期间本机笔记发生变化，已保留新内容，请再次同步');
    const next = { ...library, sync: checkpoint };
    if (!validLibrary(next)) throw new Error('同步结果格式无效，未覆盖本机笔记');
    await this.commit(next, 'sync', assets);
  }); }
  importFiles(directory = false) { return this.run(async () => { this.adopt(await invoke('library_import', { directory })); }); }
  chooseRoot() { return this.run(async () => { this.adopt(await invoke('library_choose')); }); }
  reload() {
    if (this.remote) return this.run(async () => { this.adoptServer(await this.remote!.load()); });
    if (this.external && this.disk) return this.reconcile(this.disk.root).then(() => {});
    return this.run(async () => { if (this.disk) this.adopt(await invoke('library_reload')); else await this.openFiles(); });
  }
  reconcile(root: string, buffer?: EditorBuffer) { return this.run(async () => {
    if (!this.disk || this.disk.root !== root) return { changed: false, backups: [] as string[] };
    const expectedSource = !this.external && buffer ? this.data.notes.find(note => note.id === buffer.noteId)?.source : undefined;
    const result = await invoke<{ snapshot: FileSnapshot | null; backups: string[] }>(this.external ? 'workspace_reconcile' : 'library_reconcile', {
      root, revision: this.disk.revision, buffer: buffer && expectedSource !== undefined ? { ...buffer, expectedSource } : buffer,
    });
    if (result.snapshot) { this.adopt(result.snapshot); this.externalRevision++; }
    return { changed: Boolean(result.snapshot), backups: result.backups };
  }); }
}

export type NoteOrder = { key: 'updatedAt' | 'createdAt' | 'title'; reverse: boolean; grouped: boolean };
export const defaultNoteOrder: NoteOrder = { key: 'updatedAt', reverse: false, grouped: true };
function compareNotes(order: NoteOrder, a: Note, b: Note) {
  const pinned = Number(Boolean(b.pinned && !b.trashed)) - Number(Boolean(a.pinned && !a.trashed));
  if (pinned) return pinned;
  const difference = order.key === 'title' ? noteTitle(a).localeCompare(noteTitle(b), undefined, { numeric: true }) : b[order.key] - a[order.key];
  if (!difference && order.key === 'title') return b.updatedAt - a.updatedAt;
  return order.reverse ? -difference : difference;
}

export function visibleNotes(library: Library, folderId: string | null, query: string, trashed = false, order = defaultNoteOrder) {
  const search = query.trim().toLocaleLowerCase();
  const selected = library.folders.find(folder => folder.id === folderId);
  const path = selected?.path ?? selected?.name;
  const included = new Set(library.folders.filter(folder => folder.id === folderId || path && (folder.path ?? folder.name).startsWith(path + '/')).map(folder => folder.id));
  return library.notes.filter(note => Boolean(note.trashed) === trashed && (!folderId || included.has(note.folderId)) && (!search || note.source.toLocaleLowerCase().includes(search) || noteTitle(note).toLocaleLowerCase().includes(search)))
    .sort((a, b) => compareNotes(order, a, b));
}

const localDay = (now: Date, offset: number) => new Date(now.getFullYear(), now.getMonth(), now.getDate() + offset).getTime();
const listClock = new Intl.DateTimeFormat('zh-CN', { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' });
const listWeekday = new Intl.DateTimeFormat('zh-CN', { weekday: 'long' });
const listDate = new Intl.DateTimeFormat('zh-CN', { year: 'numeric', month: 'numeric', day: 'numeric' });

export function noteListDate(now: Date) {
  const [sixDaysAgo, yesterday, today, tomorrow] = [-6, -1, 0, 1].map(offset => localDay(now, offset));
  return (timestamp: number) => {
    if (timestamp >= today && timestamp < tomorrow) return listClock.format(timestamp);
    if (timestamp >= yesterday && timestamp < today) return '昨天';
    return timestamp >= sixDaysAgo && timestamp < today ? listWeekday.format(timestamp) : listDate.format(timestamp);
  };
}

export function groupNotes(notes: Note[], order = defaultNoteOrder, now = new Date()) {
  const currentYear = now.getFullYear();
  const [today, yesterday, week, month30] = [0, -1, -7, -30].map(offset => localDay(now, offset));
  const groups: { year: number; month: number; label: string; notes: Note[] }[] = [];
  for (const note of [...notes].sort((a, b) => compareNotes(order, a, b))) {
    const timestamp = order.key === 'createdAt' ? note.createdAt : note.updatedAt;
    const date = new Date(timestamp), year = note.pinned && !note.trashed ? 0 : date.getFullYear();
    const month = year ? date.getMonth() + 1 : 0;
    const label = !year ? '置顶' : !order.grouped || order.key === 'title' ? '' : timestamp >= today ? '今天' : timestamp >= yesterday ? '昨天'
      : timestamp >= week ? '过去 7 天' : timestamp >= month30 ? '过去 30 天'
      : date.toLocaleDateString('zh-CN', { month: 'numeric', ...(year !== currentYear ? { year: 'numeric' } as const : {}) });
    const last = groups.at(-1);
    if (last?.label === label) last.notes.push(note);
    else groups.push({ year, month, label, notes: [note] });
  }
  return groups;
}
