import type { Folder, Library, Note } from './notebook';

export type ServerSnapshot = { folders: Folder[]; notes: Note[]; versions: Record<string, string>; version: string };
const endpoint = '/api/web';
const draftPrefix = 'qingyu.server-draft.';

export const isServerLibrary = () => document.querySelector('meta[name="qingyu-web-auth"][content="enabled"]') !== null;

async function request(path: string, body?: object): Promise<ServerSnapshot> {
  let response: Response;
  try {
    response = await fetch(endpoint + path, { credentials: 'same-origin', cache: 'no-store',
      ...(body ? { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) } : {}) });
  } catch { throw new Error('无法连接服务器，未保存内容仍保留，请恢复连接后重试'); }
  if (!response.ok) {
    if (response.status === 401) throw new Error('登录已过期，未保存内容仍保留，请重新登录');
    if (response.status === 412) throw new Error('服务器笔记已有新版本，未覆盖服务器内容；请先导出当前草稿再重新打开');
    if (response.status === 404) throw new Error('服务器文件已被移除，当前编辑仍保留');
    throw new Error(`服务器未能保存修改（${response.status}），当前编辑仍保留`);
  }
  const value = await response.json() as ServerSnapshot;
  if (!Array.isArray(value.folders) || !Array.isArray(value.notes) || !value.versions || typeof value.version !== 'string') throw new Error('服务器笔记库响应无效');
  return value;
}

export class ServerLibrary {
  // A page gets its own writer ID, including duplicated tabs and reloads.
  private readonly writerId = crypto.randomUUID();
  private versions: Record<string, string> = {};
  private version = '';
  load() { return request('/library'); }
  accept(snapshot: ServerSnapshot) { this.versions = snapshot.versions; this.version = snapshot.version; }
  async commit(before: Library, next: Library, intent: string) {
    const added = next.notes.filter(note => !before.notes.some(old => old.id === note.id));
    const changed = next.notes.filter(note => {
      const old = before.notes.find(old => old.id === note.id);
      return old && JSON.stringify(old) !== JSON.stringify(note);
    });
    if (!added.length && !changed.length) return undefined; // Selection is local to this browser.
    if (added.length + changed.length !== 1 || next.notes.length < before.notes.length) throw new Error('Web 每次只能提交一篇笔记的修改');
    const note = added[0] ?? changed[0];
    let body: object;
    if (added.length) {
      const duplicate = intent.startsWith('duplicate:') ? intent.slice('duplicate:'.length) : undefined;
      body = duplicate ? { kind: 'duplicateNote', id: duplicate, version: this.versions[duplicate], newId: note.id }
        : { kind: 'createNote', id: note.id, folderId: note.folderId, title: note.title ?? '', source: note.source };
    } else {
      body = { kind: 'updateNote', id: note.id, version: this.versions[note.id], source: note.source,
        title: note.title ?? null, pinned: Boolean(note.pinned), trashed: Boolean(note.trashed), folderId: note.folderId };
    }
    const snapshot = await request('/commands', body);
    this.clearDraft(note.id, note.source);
    return snapshot;
  }
  createFolder(parentId: string, name: string) { return request('/commands', { kind: 'createFolder', parentId, name }); }
  folder(action: 'rename' | 'move', id: string, value: string) {
    return request('/commands', { kind: action === 'rename' ? 'renameFolder' : 'moveFolder', id, version: this.version,
      ...(action === 'rename' ? { name: value } : { destinationId: value }) });
  }
  private draftKey(id: string) { return `${draftPrefix}${id}.${this.writerId}`; }
  stageDraft(id: string, source: string, savedSource: string) {
    const key = this.draftKey(id);
    // Retain an undo to the old baseline if an earlier save is still in flight.
    if (source.replace(/\r\n?/g, '\n') === savedSource.replace(/\r\n?/g, '\n') && !localStorage.getItem(key)) return;
    localStorage.setItem(key, JSON.stringify({ id, source, updatedAt: Date.now() }));
  }
  private clearDraft(id: string, source: string) {
    try {
      const key = this.draftKey(id);
      const raw = localStorage.getItem(key);
      if (raw && JSON.parse(raw).source === source) localStorage.removeItem(key);
    } catch { /* A backup cleanup failure must not turn an acknowledged server save into a failure. */ }
  }
}

export function offerServerDrafts(library: Library) {
  const drafts: { id: string; source: string }[] = [];
  for (let i = 0; i < localStorage.length; i++) {
    const key = localStorage.key(i)!;
    if (!key.startsWith(draftPrefix)) continue;
    try { const draft = JSON.parse(localStorage.getItem(key)!); if (typeof draft.id === 'string' && typeof draft.source === 'string' && library.notes.find(note => note.id === draft.id)?.source !== draft.source) drafts.push(draft); } catch { /* Keep unrecognized local data untouched. */ }
  }
  if (!drafts.length) return;
  const dialog = document.createElement('dialog'); dialog.setAttribute('aria-label', '未保存的草稿');
  const text = document.createElement('p'); text.textContent = `此浏览器有 ${drafts.length} 份未提交草稿。请先下载对比，服务器内容不会自动被覆盖。`;
  dialog.append(text);
  for (const [i, draft] of drafts.entries()) {
    const button = document.createElement('button'); button.textContent = `下载草稿 ${i + 1}`;
    button.onclick = () => {
      const link = document.createElement('a'); link.href = URL.createObjectURL(new Blob([draft.source], { type: 'text/markdown;charset=utf-8' }));
      link.download = `轻羽草稿-${i + 1}.md`; link.click(); setTimeout(() => URL.revokeObjectURL(link.href), 30_000);
    };
    dialog.append(button);
  }
  const close = document.createElement('button'); close.textContent = '稍后处理'; close.onclick = () => dialog.close(); dialog.append(close);
  dialog.addEventListener('close', () => dialog.remove()); document.body.append(dialog); dialog.showModal();
}
