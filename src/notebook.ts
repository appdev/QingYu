import { noteText, noteTitle, replaceFirstTitleLine } from './document-text';
import { dailyFolderName, dailyNoteId, dailyTitle, findDailyNote, type DailyNoteSettings } from './daily-note';
import type { State } from './vendor/mdsync/core';
import { invoke } from '@tauri-apps/api/core';
import type { AssetPlaceholders, AssetStat, AssetSync, StagedAsset } from './sync-files';
import { ServerLibrary, ServerVersionConflict, type ServerSnapshot } from './server-library';
import { uiText } from './ui-language';
import { retargetNoteLinks } from './note-links';

export const libraryKey = 'miaoyan-editor.notebook.v1';
export const legacyDraftKey = 'miaoyan-editor.draft.v1';
export type Folder = { id: string; name: string; path?: string };
// 分享状态只记在元数据里；没有 share 等同于未分享。
export type NoteShare = { status: 'shared' | 'unshared'; url?: string };
// followTitle records the last confirmed filename stem before collision suffixes.
export type Note = { id: string; folderId: string; source: string; createdAt: number; updatedAt: number; conflictOf?: string; title?: string; pinned?: boolean; trashed?: boolean; path?: string; filename?: string; readOnly?: boolean; share?: NoteShare; followTitle?: string };

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
type LocalNoteName = { title: string | null; filename: string | null; localTitle: string; localFilename: string };
export type Checkpoint = { target: string; state: State; cloudBinding?: string; localNoteNames?: Record<string, LocalNoteName> } & ({ version: 1 } | {
  version: 2; placeholderCount: number; placeholderDigest?: string;
});
export type Library = { version: 1; folders: Folder[]; notes: Note[]; activeId: string; sync?: Checkpoint };
export type EditorBuffer = { noteId: string; source: string };
export type NoteVersion = { id: string; updatedAt: number };
type FileSnapshot = { library: Library; root: string; revision: number; trashed_folders?: string[] };
type Storage = Pick<globalThis.Storage, 'getItem' | 'setItem'>;
export class SyncSnapshotChanged extends Error {
  constructor() { super('同步期间本机笔记发生变化，已保留新内容，请再次同步'); }
}

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
        (note.share !== undefined && !validShare(note.share)) ||
        (note.followTitle !== undefined && (typeof note.followTitle !== 'string' || note.followTitle.length > 500))) return false;
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
  onLocalChange?: () => void;
  trashedFolders: readonly string[] = [];
  onSyncApply?: () => (applied: boolean) => void;
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
  get hasNoteHistory() { return Boolean((this.disk || this.remote) && !this.external && !this.loadError); }
  noteVersions(noteId: string) {
    return this.remote?.noteVersions(noteId) ?? invoke<NoteVersion[]>('library_note_versions', { noteId });
  }
  noteVersion(noteId: string, version: string) {
    return this.remote?.noteVersion(noteId, version) ?? invoke<string>('library_note_version', { noteId, version });
  }
  restoreNoteVersion(noteId: string, version: string, expectedSource: string, root: string | undefined) { return this.run(async () => {
    if (!this.hasNoteHistory || root !== this.root || this.current?.id !== noteId) throw new Error('原笔记已切换，请重新打开版本历史');
    if (this.remote) {
      if (this.current.source !== expectedSource) throw new Error('笔记正文已更新，请重新打开版本历史');
      this.adoptServer(await this.remote.restoreNoteVersion(noteId, version));
    } else {
      this.adopt(await invoke<FileSnapshot>('library_restore_note_version', { root, noteId, version, expectedSource }));
    }
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
    this.trashedFolders = snapshot.trashedFolders ?? [];
  }
  /** 文件夹和废纸篓操作作用于整个笔记库；部署端刚同步过时先读回最新内容再做一次，和 App 总是基于最新笔记库操作一致。 */
  private async changeServer(change: () => Promise<ServerSnapshot>) {
    try { this.adoptServer(await change()); }
    catch (error) {
      if (!(error instanceof ServerVersionConflict)) throw error;
      this.adoptServer(await this.remote!.load());
      this.adoptServer(await change());
    }
  }
  stageDraft(source: string) { if (this.current) this.remote?.stageDraft(this.current.id, source, this.current.source); }
  refreshServer(ready: () => boolean, preferredId?: string) { return this.run(async () => {
    if (!this.remote || !ready()) return false;
    const snapshot = await this.remote.load();
    if (!ready()) return false;
    const previous = JSON.stringify(this.data); this.adoptServer(snapshot, preferredId);
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
    this.trashedFolders = snapshot.trashed_folders ?? [];
  }
  private run<T>(action: () => Promise<T>): Promise<T> {
    const operation = this.pending.then(action); this.pending = operation.catch(() => undefined); return operation;
  }
  private async commit(next: Library, intent = 'edit', assets?: AssetSync) {
    if (this.loadError) throw new Error('笔记库读取失败，未覆盖原有数据');
    const changed = intent !== 'sync' && (next.notes !== this.data.notes || next.folders !== this.data.folders);
    if (this.remote) {
      const snapshot = await this.remote.commit(this.data, next, intent);
      if (snapshot) this.adoptServer(snapshot, next.activeId); else this.data = next;
    }
    else if (this.disk) this.adopt(await invoke(this.external ? 'workspace_commit' : 'library_commit', { next, intent, revision: this.disk.revision, ...(assets ? { assets } : {}) }), this.noteWindow ? next.activeId : undefined);
    else { this.storage.setItem(libraryKey, JSON.stringify(next)); this.data = next; this.persisted = true; }
    if (changed) this.onLocalChange?.();
  }
  save(source: string, id = this.data.activeId, expectedSource?: string, confirmTitle = false) { const externalRevision = this.externalRevision, root = this.disk?.root; return this.run(async () => {
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
    if (confirmTitle && !current.trashed && !current.readOnly && !this.external) {
      const next = this.titleFromSource(current, source);
      if (next.followTitle !== current.followTitle) {
        await this.commitTitle(current, next);
        return;
      }
    }
    if (!changed && this.persisted) return;
    if (root && !this.external && !this.remote && changed) {
      if (this.loadError) throw new Error('笔记库读取失败，未覆盖原有数据');
      this.adopt(await invoke('library_save_note', { root, noteId: id, expectedSource: current.source, source }));
      this.onLocalChange?.();
      return;
    }
    await this.commit(changed ? { ...this.data, notes: this.data.notes.map(note => note.id === current.id ? { ...note, source, updatedAt: Date.now() } : note) } : this.data);
  }); }
  saveDraftAfterSync(note: Note, source: string, applied?: Note) { return this.run(async () => {
    const current = this.data.notes.find(entry => entry.id === note.id);
    if (current?.readOnly) throw new Error('文件内容损坏或不可读，已阻止覆盖');
    if (current?.source === source && !current.trashed) return;
    const now = Date.now();
    const folderId = current?.folderId ?? this.data.folders.find(folder => folder.id === note.folderId)?.id ?? this.data.folders[0]!.id;
    const recovered = { ...(current ?? note), folderId, source, updatedAt: now, trashed: false };
    const notes = current
      ? this.data.notes.map(entry => entry.id === note.id ? recovered : entry)
      : [...this.data.notes, recovered];
    if (current && current.source === applied?.source && current.source !== source && current.source !== note.source) {
      notes.push({ id: crypto.randomUUID(), folderId, source: current.source, createdAt: now, updatedAt: now,
        title: `${noteTitle(current).slice(0, 145)}（冲突副本）`, conflictOf: current.id });
    }
    await this.commit({ ...this.data, notes, activeId: note.id });
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
  folderAction(action: 'rename' | 'move' | 'move-to-folder' | 'duplicate' | 'trash' | 'trash-local' | 'restore-folder' | 'reveal' | 'terminal', folderId: string | null, value?: string) { return this.run(async () => {
    if (this.remote) {
      if (action === 'duplicate' || action === 'move-to-folder' || action === 'trash-local' || action === 'restore-folder') {
        const remote = this.remote;
        await this.changeServer(() => remote.folderAction(action, folderId, value));
      } else {
        // 在 Finder 或终端中打开需要本机系统，网页客户端不提供；桌面端的系统废纸篓在网页上用笔记库废纸篓代替
        if (!folderId || !value || !['rename', 'move'].includes(action)) throw new Error('网页客户端不支持此文件夹操作');
        const remote = this.remote;
        await this.changeServer(() => remote.folder(action as 'rename' | 'move', folderId, value));
      }
      this.onLocalChange?.();
      return;
    }
    if (this.external) throw new Error('临时文件模式不管理文件夹，请返回笔记库操作');
    if (this.disk) {
      const revision = this.disk.revision;
      this.adopt(await invoke('library_folder_action', { action, folderId, value, revision }));
      if (this.disk.revision !== revision) this.onLocalChange?.();
      return;
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
    const note = this.data.notes.find(note => note.id === id);
    if (!note) throw new Error('笔记不存在');
    if (this.external) return this.updateNote(id, { title });
    if (note.readOnly || note.trashed) throw new Error('当前笔记不可重命名');
    if (!noteText(title, '').title) throw new Error('请输入有效的标题文字');
    const source = replaceFirstTitleLine(note.source, title);
    const next = this.titleFromSource(note, source, false);
    if (note.followTitle !== next.followTitle || note.title !== next.title) await this.commitTitle(note, next);
    else if (note.source !== source) await this.commit({
      ...this.data, notes: this.data.notes.map(other => other.id === id ? { ...next, updatedAt: Date.now() } : other),
    });
  }); }
  private titleFromSource(note: Note, source: string, avoidCollisions = true): Note {
    const firstLine = noteText(source, '').title;
    let stem = firstLine.replace(/[<>:"/\\|?*#[\]\p{Cc}]/gu, '-').replace(/^[.\s]+|[.\s]+$/gu, '');
    const characters = [...stem];
    const encoder = new TextEncoder();
    let bytes = encoder.encode(stem).length;
    while (bytes > 220) bytes -= encoder.encode(characters.pop()!).length;
    stem = characters.join('').replace(/[.\s]+$/gu, '');
    if (/^(?:con|prn|aux|nul|com[1-9¹²³]|lpt[1-9¹²³])(?:\.|$)/iu.test(stem)) stem = `_${stem}`;
    if (!stem || stem === note.followTitle) return { ...note, source };
    const ext = (note.filename ?? note.path)?.match(/\.[^.]+$/u)?.[0] ?? '.md';
    const key = (value: string) => value.normalize('NFC').toLowerCase();
    const occupied = new Set(this.data.notes.filter(other => other.id !== note.id && other.folderId === note.folderId && !other.trashed)
      .flatMap(other => [key(other.filename ?? ''), key(`${other.title ?? ''}${ext}`)]));
    let title = stem;
    for (let suffix = 2; avoidCollisions && occupied.has(key(title + ext)); suffix++) title = `${stem} ${suffix}`;
    return { ...note, source, title, followTitle: stem };
  }
  private async commitTitle(before: Note, note: Note) {
    const time = Date.now();
    const renamed = note.title !== before.title && note.title !== undefined;
    const notes = this.data.notes.map(other => {
      const current = other.id === note.id ? note : other;
      const source = renamed && !current.readOnly && !current.trashed ? retargetNoteLinks(this.data, before, note.title!, current) : current.source;
      return source !== other.source ? { ...current, source, updatedAt: time } : current;
    });
    await this.commit({ ...this.data, notes }, 'title-edit');
  }
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
  purgeNotes(ids: readonly string[]) { return this.run(async () => {
    if (this.remote) { const remote = this.remote; await this.changeServer(() => remote.purgeNotes(ids)); this.onLocalChange?.(); return; }
    if (!this.disk || this.external || this.noteWindow || this.loadError) throw new Error('请在本地笔记库的废纸篓里永久删除');
    this.adopt(await invoke('library_purge_notes', { noteIds: ids, root: this.disk.root, revision: this.disk.revision }));
    this.onLocalChange?.();
  }); }
  duplicateNote(id: string) { return this.run(async () => {
    const note = this.data.notes.find(note => note.id === id);
    if (!note) throw new Error('笔记不存在');
    if (note.readOnly) throw new Error('文件不可读，无法复制');
    // MiaoYan getDupeName uses " Copy", then " Copy 2", without changing the body.
    const base = (note.filename ? note.filename.replace(/\.[^.]+$/, '') : noteTitle(note)).replace(/ Copy(?: \d+)?$/, '').slice(0, 145);
    let title = `${base} Copy`, number = 2;
    while (this.data.notes.some(other => other.folderId === note.folderId && (other.title ?? noteTitle(other)) === title)) title = `${base} Copy ${number++}`;
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
  commitSync(library: Library, checkpoint: Checkpoint, expected: Library, assets?: AssetSync,
    location = { root: this.root, revision: this.assetRevision }, published?: State, ready = true) { return this.run(async () => {
    if (this.external) throw new Error('外部文件不在 S3 同步范围内');
    if (this.remote) throw new Error('Web 使用服务器笔记库，S3 同步请在桌面端操作');
    if (this.root !== location.root) throw new SyncSnapshotChanged();
    const current = this.data;
    if (!ready || JSON.stringify({ ...current, activeId: expected.activeId }) !== JSON.stringify(expected)
      || (current.activeId === expected.activeId && this.assetRevision !== location.revision)) {
      // Record what this device published without acknowledging remote changes it has yet to apply.
      if (published && JSON.stringify(current.sync) === JSON.stringify(expected.sync)) {
        const previous = current.sync?.target === checkpoint.target ? current.sync : undefined;
        await this.commit({ ...current, sync: { ...(previous ?? { version: 1, target: checkpoint.target, cloudBinding: checkpoint.cloudBinding }), state: published } });
      }
      if (!ready) throw new Error('当前输入尚未保存，已保留本机内容，请完成输入后再次同步');
      throw new SyncSnapshotChanged();
    }
    const activeId = current.activeId !== expected.activeId && library.notes.some(note => note.id === current.activeId && !note.trashed)
      ? current.activeId : library.activeId;
    const next = { ...library, activeId, sync: checkpoint };
    if (!validLibrary(next)) throw new Error('同步结果格式无效，未覆盖本机笔记');
    const finish = this.onSyncApply?.();
    let applied = false;
    try {
      await this.commit(next, 'sync', assets);
      applied = true;
    } finally { finish?.(applied); }
  }); }
  importFiles(directory = false) { return this.run(async () => {
    const revision = this.disk?.revision;
    this.adopt(await invoke('library_import', { directory }));
    if (this.disk?.revision !== revision) this.onLocalChange?.();
  }); }
  /** 网页客户端导入用户在浏览器里选择的文件，由部署端用和 App 相同的导入逻辑写入笔记库。 */
  importUploads(files: readonly File[]) { return this.run(async () => {
    if (!this.remote) throw new Error('请在应用中导入本地文件');
    const markdown = files.filter(file => /\.(md|markdown|txt)$/i.test(file.name));
    if (!markdown.length) throw new Error('没有可导入的 Markdown 文件');
    const uploads = await Promise.all(markdown.map(async file => {
      const bytes = new Uint8Array(await file.arrayBuffer());
      let binary = '';
      for (let offset = 0; offset < bytes.length; offset += 0x8000) binary += String.fromCharCode(...bytes.subarray(offset, offset + 0x8000));
      return { path: file.webkitRelativePath || file.name, data: btoa(binary) };
    }));
    const previous = new Set(this.data.notes.map(note => note.id));
    const snapshot = await this.remote.importFiles(uploads);
    // 和 App 一样，导入后打开最后导入的那篇
    const imported = snapshot.notes.filter(note => !previous.has(note.id)).at(-1);
    this.adoptServer(snapshot, imported?.id);
    this.onLocalChange?.();
  }); }
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
export function noteListDate(now: Date, locale = 'zh-CN') {
  const listClock = new Intl.DateTimeFormat(locale, { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' });
  const listWeekday = new Intl.DateTimeFormat(locale, { weekday: 'long' });
  const listDate = new Intl.DateTimeFormat(locale, { year: 'numeric', month: 'numeric', day: 'numeric' });
  const yesterdayLabel = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' }).format(-1, 'day');
  const [sixDaysAgo, yesterday, today, tomorrow] = [-6, -1, 0, 1].map(offset => localDay(now, offset));
  return (timestamp: number) => {
    if (timestamp >= today && timestamp < tomorrow) return listClock.format(timestamp);
    if (timestamp >= yesterday && timestamp < today) return yesterdayLabel;
    return timestamp >= sixDaysAgo && timestamp < today ? listWeekday.format(timestamp) : listDate.format(timestamp);
  };
}

export function groupNotes(notes: Note[], order = defaultNoteOrder, now = new Date(), locale = 'zh-CN') {
  const currentYear = now.getFullYear();
  const [today, yesterday, week, month30] = [0, -1, -7, -30].map(offset => localDay(now, offset));
  const yesterdayLabel = new Intl.RelativeTimeFormat(locale, { numeric: 'auto' }).format(-1, 'day');
  const pastDays = (count: number) => uiText('pastDays', locale).replace('{count}', count.toLocaleString(locale));
  const groups: { year: number; month: number; label: string; notes: Note[] }[] = [];
  for (const note of [...notes].sort((a, b) => compareNotes(order, a, b))) {
    const timestamp = order.key === 'createdAt' ? note.createdAt : note.updatedAt;
    const date = new Date(timestamp), year = note.pinned && !note.trashed ? 0 : date.getFullYear();
    const month = year ? date.getMonth() + 1 : 0;
    const label = !year ? uiText('pin', locale) : !order.grouped || order.key === 'title' ? '' : timestamp >= today ? uiText('menu.today', locale) : timestamp >= yesterday ? yesterdayLabel
      : timestamp >= week ? pastDays(7) : timestamp >= month30 ? pastDays(30)
      : date.toLocaleDateString(locale, { month: 'numeric', ...(year !== currentYear ? { year: 'numeric' } as const : {}) });
    const last = groups.at(-1);
    if (last?.label === label) last.notes.push(note);
    else groups.push({ year, month, label, notes: [note] });
  }
  return groups;
}
