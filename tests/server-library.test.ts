// @vitest-environment jsdom
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { Notebook, type Library } from '../src/notebook';
import { isServerLibrary, type ServerSnapshot } from '../src/server-library';
import { webcrypto } from 'node:crypto';
import { dailyNoteId } from '../src/daily-note';
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(() => { throw new Error('Native transport must not be used'); }) }));
const fetchMock = vi.fn();
const note = { id: 'one', folderId: 'default', source: '# Server', createdAt: 1, updatedAt: 1, title: 'Server' };
function snapshot(source = note.source, version = 'v1'): ServerSnapshot {
  return { folders: [{ id: 'default', name: '备忘录', path: '' }], notes: [{ ...note, source }], versions: { one: version }, version: 'library-' + version };
}
const ok = (value: ServerSnapshot) => ({ ok: true, json: async () => value });
const drafts = () => Object.keys(localStorage).filter(key => key.startsWith('qingyu.server-draft.')).map(key => JSON.parse(localStorage.getItem(key)!));
function book() { return new Notebook({ getItem: () => null, setItem: () => { throw new Error('Do not store server notes in localStorage'); } }, ''); }
beforeEach(() => { localStorage.clear(); document.head.innerHTML = ''; fetchMock.mockReset(); vi.stubGlobal('fetch',fetchMock); });
afterEach(() => vi.unstubAllGlobals());

it('loads server files without migrating browser notes and keeps selection local', async () => {
  localStorage.setItem('miaoyan-editor.notebook.v1','browser content');
  document.head.innerHTML='<meta name="qingyu-web-auth" content="enabled">';expect(isServerLibrary()).toBe(true);
  const data=snapshot();data.notes.push({...note,id:'two'});data.versions.two='v2';
  fetchMock.mockResolvedValueOnce(ok(data));const notebook=book();await notebook.openServer();
  expect(notebook.server).toBe(true);expect(notebook.current.source).toBe('# Server');
  await notebook.select('two');expect(fetchMock).toHaveBeenCalledTimes(1);
  expect(localStorage.getItem('miaoyan-editor.notebook.v1')).toBe('browser content');
});

it('commits only the changed note with its version and keeps newer pending drafts after an earlier save', async () => {
  fetchMock.mockResolvedValueOnce(ok(snapshot()));const notebook=book();await notebook.openServer();
  let acknowledge!: (value: unknown) => void;
  fetchMock.mockImplementationOnce(()=>new Promise(resolve=>{acknowledge=resolve;}));
  notebook.stageDraft('# B');const saving=notebook.save('# B');await Promise.resolve();
  notebook.stageDraft('# C');acknowledge(ok(snapshot('# B','v2')));await saving;
  const body=JSON.parse(fetchMock.mock.calls[1][1].body);
  expect(body).toMatchObject({kind:'updateNote',id:'one',version:'v1',source:'# B',share:null});expect(body).not.toHaveProperty('notes');
  expect(drafts().map(draft => draft.source)).toEqual(['# C']);
  fetchMock.mockResolvedValueOnce(ok(snapshot('# C','v3')));await notebook.save('# C');
  expect(drafts()).toEqual([]);
});

it('keeps old versions and draft content on conflicts, and discards refreshes racing with typing', async () => {
  fetchMock.mockResolvedValueOnce(ok(snapshot()));const notebook=book();await notebook.openServer();
  fetchMock.mockResolvedValueOnce({ok:false,status:412});notebook.stageDraft('My draft');
  await expect(notebook.save('My draft')).rejects.toThrow('已有新版本');
  expect(notebook.current.source).toBe('# Server');expect(drafts().map(draft => draft.source)).toEqual(['My draft']);
  let clean=true;fetchMock.mockImplementationOnce(async()=>{clean=false;return ok(snapshot('Remote','v99'));});
  expect(await notebook.refreshServer(()=>clean)).toBe(false);expect(notebook.current.source).toBe('# Server');
  fetchMock.mockResolvedValueOnce(ok(snapshot('Saved','v2')));await notebook.save('Saved');
  expect(JSON.parse(fetchMock.mock.calls.at(-1)![1].body).version).toBe('v1');
});

it('uses shared folder commands and never falls back to browser storage when the server is unavailable', async () => {
  fetchMock.mockResolvedValueOnce(ok(snapshot()));const notebook=book();await notebook.openServer();
  const data=snapshot();data.folders.push({id:'new-folder',name:'项目',path:'项目'});
  fetchMock.mockResolvedValueOnce(ok(data));expect(await notebook.createFolder('项目')).toBe('new-folder');
  expect(JSON.parse(fetchMock.mock.calls[1][1].body)).toEqual({kind:'createFolder',parentId:'default',name:'项目'});
  fetchMock.mockRejectedValueOnce(new Error('offline'));await expect(notebook.save('offline draft')).rejects.toThrow('无法连接');
  expect(notebook.current.source).toBe('# Server');
});

it('opens the other tab’s daily note after racing both the folder and note writes', async () => {
  vi.stubGlobal('crypto', webcrypto);
  const now = new Date(2026, 10, 16, 10);
  const dayFolder = { id: 'daily', name: '每日笔记', path: '每日笔记' };
  const id = await dailyNoteId(dayFolder.id, now, 0);
  const folderExists = snapshot(); folderExists.folders.push(dayFolder);
  const noteExists = structuredClone(folderExists);
  noteExists.notes.push({ ...note, id, folderId: dayFolder.id, source: '# 2026-11-16\n', title: '2026-11-16', createdAt: now.getTime() });
  fetchMock.mockResolvedValueOnce(ok(snapshot()))
    .mockResolvedValueOnce({ ok: false, status: 409 }).mockResolvedValueOnce(ok(folderExists))
    .mockResolvedValueOnce({ ok: false, status: 400 }).mockResolvedValueOnce(ok(noteExists));
  const notebook = book(); await notebook.openServer();
  await expect(notebook.openToday({ todayFolder: '', todayTitle: 'iso' }, now)).resolves.toBe(false);
  expect(notebook.current.id).toBe(id);
  expect(fetchMock).toHaveBeenCalledTimes(5);
});

it('isolates drafts and successful-save cleanup across tabs, including identical text', async () => {
  fetchMock.mockResolvedValue(ok(snapshot()));
  const a = book(), b = book(); await a.openServer(); await b.openServer();
  a.stageDraft('Pending in A'); b.stageDraft('Pending in B');
  expect(drafts().map(draft => draft.source).sort()).toEqual(['Pending in A', 'Pending in B']);
  b.stageDraft('# Server');
  expect(drafts().some(draft => draft.source === 'Pending in A')).toBe(true);
  b.stageDraft('Pending in A');
  fetchMock.mockResolvedValueOnce(ok(snapshot('Pending in A', 'v2'))); await b.save('Pending in A');
  expect(drafts().map(draft => draft.source)).toEqual(['Pending in A']);
  const before = localStorage.length; b.stageDraft('Pending in A');
  expect(localStorage.length).toBe(before);
});

it('preserves legacy recovery records and records undo to the old baseline while an earlier save is pending', async () => {
  const legacy = { id: 'one', source: 'Legacy recovery draft', updatedAt: 1 };
  localStorage.setItem('qingyu.server-draft.one', JSON.stringify(legacy));
  fetchMock.mockResolvedValueOnce(ok(snapshot())); const notebook = book(); await notebook.openServer();
  notebook.stageDraft('# Server'); expect(drafts()).toEqual([legacy]);
  let acknowledge!: (value: unknown) => void;
  fetchMock.mockImplementationOnce(() => new Promise(resolve => { acknowledge = resolve; }));
  notebook.stageDraft('# B'); const saving = notebook.save('# B'); await Promise.resolve();
  notebook.stageDraft('# Server'); acknowledge(ok(snapshot('# B', 'v2'))); await saving;
  expect(drafts().map(draft => draft.source).sort()).toEqual(['# Server', 'Legacy recovery draft']);
});

it('retries folder and trash operations once on the latest library after a deployment sync changed it', async () => {
  fetchMock.mockResolvedValueOnce(ok(snapshot()));const notebook=book();await notebook.openServer();
  const synced={...snapshot('# Server','v2'),trashedFolders:['归档']};
  fetchMock.mockResolvedValueOnce({ok:false,status:412,json:async()=>({})});
  fetchMock.mockResolvedValueOnce(ok(synced));
  fetchMock.mockResolvedValueOnce(ok({...snapshot('# Server','v2'),version:'library-v3',trashedFolders:[]}));
  await notebook.folderAction('restore-folder',null,'归档');
  const restore=JSON.parse(fetchMock.mock.calls[3][1].body);
  expect(restore).toMatchObject({kind:'folderAction',action:'restore-folder',value:'归档',version:'library-v2'});
  expect(notebook.trashedFolders).toEqual([]);
  fetchMock.mockResolvedValueOnce({ok:false,status:409,json:async()=>({error:{code:'library_conflict',message:'笔记已变化，只能永久删除仍在废纸篓里的笔记'}})});
  await expect(notebook.purgeNotes(['one'])).rejects.toThrow('只能永久删除仍在废纸篓里的笔记');
});

it('imports picked markdown files through the deployment and syncs afterwards', async () => {
  fetchMock.mockResolvedValueOnce(ok(snapshot()));const notebook=book();await notebook.openServer();
  const changed=vi.fn();notebook.onLocalChange=changed;
  const imported=snapshot();imported.notes.push({...note,id:'two',source:'# 京都'});imported.versions.two='v1';
  fetchMock.mockResolvedValueOnce(ok(imported));
  // jsdom 的 File 没有 arrayBuffer()，浏览器都有；这里按浏览器行为补上
  const withBytes=(file:File)=>Object.assign(file,{arrayBuffer:()=>new Promise<ArrayBuffer>(resolve=>{const reader=new FileReader();reader.onload=()=>resolve(reader.result as ArrayBuffer);reader.readAsArrayBuffer(file);})});
  const kyoto=withBytes(new File(['# 京都'],'京都.md'));Object.defineProperty(kyoto,'webkitRelativePath',{value:'旅行/京都.md'});
  await notebook.importUploads([kyoto,new File(['png'],'封面.png')]);
  const [path,init]=fetchMock.mock.calls[1];
  expect(path).toBe('/api/web/import');
  expect(JSON.parse(init.body)).toEqual([{path:'旅行/京都.md',data:btoa(unescape(encodeURIComponent('# 京都')))}]);
  expect(notebook.data.notes.map(item=>item.source)).toContain('# 京都');
  expect(notebook.current.source).toBe('# 京都');
  expect(changed).toHaveBeenCalledOnce();
  await expect(notebook.importUploads([new File(['png'],'封面.png')])).rejects.toThrow('没有可导入的 Markdown 文件');
});
