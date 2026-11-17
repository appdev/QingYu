import type { Folder, Library, Notebook } from './notebook';
import * as core from './vendor/mdsync/core';
import { SyncFiles, scope, ownedPath, notePath, metaPath, folderPath, assetPath, assetRelative, MAX_ASSET_BYTES, MAX_ASSETS_TOTAL, type NoteMeta } from './sync-files';
import { createStore, normalizeSettings, openKey, type S3Connection } from './sync-store';

const textEncoder = new TextEncoder();
const parse = <T>(text: string | null): T => { if (text === null) throw new Error('远端同步文件缺失'); return JSON.parse(text) as T; };
async function copyId(id: string, token: string) { return `conflict-${(await core.sha256Hex(textEncoder.encode(`${id}\n${token}`))).slice(0, 40)}`; }
function checkHead(head: core.Head | null) {
  if (!head) return;
  if (head.version !== 3 || !head.files || Object.keys(head.files).length > 30000 || Object.entries(head.files).some(([path, entry]) =>
    !ownedPath(path) || !/^[a-f0-9]{64}$/.test(entry.hash) || !Number.isFinite(entry.size) || entry.size < 0 || entry.size > (path.startsWith('assets/') ? MAX_ASSET_BYTES : 4 * 1024 * 1024) ||
    !Number.isFinite(entry.mtime) || !Number.isFinite(new Date(entry.mtime).getTime()))) throw new Error('远端笔记库协议或文件格式无效，已停止同步');
  if (Object.entries(head.files).reduce((sum, [path, entry]) => sum + (path.startsWith('assets/') ? entry.size : 0), 0) > MAX_ASSETS_TOTAL) throw new Error('远端附件快照超过 256 MiB，已停止同步');
}

async function preserveConflicts(files: SyncFiles, deps: core.Dependencies, result: core.Comparison) {
  const remote = result.remote;
  if (!remote) throw new Error('冲突缺少远端版本');
  const base = deps.state.baseline ? structuredClone(deps.state.baseline) : { ...remote, files: {} };
  const remoteText = async (path: string) => remote.files[path] ? core.loadRemoteText(deps, remote.files[path]!.hash) : null;
  const acknowledge = (path: string) => {
    if (remote.files[path]) base.files[path] = remote.files[path]!;
    else delete base.files[path];
  };
  for (const conflict of result.diff.conflicts.filter(c => c.path.startsWith('assets/'))) {
    const path = conflict.path, local = files.files.get(path), other = remote.files[path];
    if (other) {
      // Download into a separate adapter: binary conflicts must never overwrite the local copy.
      const incoming = new SyncFiles({ version: 1, notes: [], folders: [], activeId: '' });
      const isolated = { ...deps, adapter: incoming, state: { ...structuredClone(deps.state), vaultId: remote.vaultId, baseline: null, hashCache: {} } as core.State };
      const context: core.Context = { persistState: async state => { isolated.state = state; }, getFreshState: () => isolated.state,
        setProgress() {}, reportProgressSoon() {}, logInfo: async () => {} };
      const download = await core.compare(isolated, remote);
      await core.pullPathsOp(isolated, download, [path], context);
      const value = incoming.files.get(path);
      if (!value) throw new Error('附件冲突版本未完整下载，已停止同步');
      let destination = path;
      if (local) {
        const relative = assetRelative(path), slash = relative.lastIndexOf('/'), dot = relative.lastIndexOf('.');
        const split = dot > slash + 1 ? dot : relative.length;
        destination = assetPath(`${relative.slice(0, split)}（冲突-${other.hash.slice(0, 12)}）${relative.slice(split)}`);
        const existing = files.files.get(destination);
        if (existing && await core.sha256Hex(existing.bytes) !== await core.sha256Hex(value.bytes)) throw new Error('附件冲突副本名称已被占用，未覆盖文件');
      }
      files.set(destination, value.bytes, other.mtime);
    }
    acknowledge(path);
  }
  // Folder metadata is separate from note bodies: a rename cannot turn into a text merge.
  for (const conflict of result.diff.conflicts.filter(c => c.path.startsWith('library/folders/'))) {
    const local = files.files.has(conflict.path) ? parse<Folder>(await files.read(conflict.path)) : null;
    const otherText = await remoteText(conflict.path), other = otherText ? parse<Folder>(otherText) : null;
    if (local && other && (conflict.path !== folderPath(local.id) || local.id !== other.id)) throw new Error('文件夹冲突元数据无效');
    if (local && other && local.name !== other.name) {
      const id = await copyId(local.id, conflict.remoteHash + JSON.stringify(local));
      files.put(folderPath(id), JSON.stringify({ id, name: `${local.name}（冲突副本）` }), 1);
      for (const path of files.files.keys()) {
        if (!path.startsWith('library/notes/')) continue;
        const note = parse<NoteMeta>(await files.read(path));
        if (note.folderId === local.id) files.put(path, JSON.stringify({ ...note, folderId: id }), note.createdAt);
      }
      files.put(conflict.path, otherText!, 1);
    } else if (!local && other) files.put(conflict.path, otherText!, 1);
    acknowledge(conflict.path);
  }
  const ids = new Set<string>();
  for (const conflict of result.diff.conflicts.filter(c => !c.path.startsWith('library/folders/') && !c.path.startsWith('assets/'))) {
    const localMetaPath = conflict.path.startsWith('notes/') ? conflict.path.replace(/^notes\//, 'library/notes/').replace(/\.md$/, '.json') : conflict.path;
    const remoteMeta = await remoteText(localMetaPath);
    const local = files.files.has(localMetaPath) ? parse<NoteMeta>(await files.read(localMetaPath)) : null;
    const other = remoteMeta ? parse<NoteMeta>(remoteMeta) : null;
    const id = local?.id ?? other?.id;
    if (!id || localMetaPath !== metaPath(id) || (local && other && local.id !== other.id)) throw new Error('笔记冲突元数据无效');
    ids.add(id);
  }
  for (const id of ids) {
    const bodyPath = notePath(id), metadata = metaPath(id);
    const local = files.files.has(metadata) ? parse<NoteMeta>(await files.read(metadata)) : null;
    const other = remote.files[metadata] ? parse<NoteMeta>(await remoteText(metadata)) : null;
    if (!!other !== !!remote.files[bodyPath]) throw new Error('远端笔记正文与元数据不完整');
    if (other) {
      const source = await remoteText(bodyPath);
      if (source === null || other.id !== id) throw new Error('远端笔记内容无效');
      if (local && files.files.has(bodyPath)) {
        const duplicate = await copyId(id, `${remote.files[bodyPath]!.hash}:${remote.files[metadata]!.hash}`);
        const meta: NoteMeta = { ...other, id: duplicate, conflictOf: id };
        files.put(metaPath(duplicate), JSON.stringify(meta), other.createdAt);
        files.put(notePath(duplicate), source, remote.files[bodyPath]!.mtime);
      } else {
        files.put(metadata, JSON.stringify(other), other.createdAt);
        files.put(bodyPath, source, remote.files[bodyPath]!.mtime);
      }
    }
    // Only these explicitly resolved paths adopt the remote base; unread notes are never claimed.
    acknowledge(bodyPath); acknowledge(metadata);
  }
  deps.state = { ...deps.state, vaultId: remote.vaultId, baseline: base, hashCache: {} };
  return result.diff.conflicts.length;
}

export async function syncNotebook(book: Notebook, settings: S3Connection, progress: (message: string) => void = () => undefined, beforeCommit: () => boolean | Promise<boolean> = () => true) {
  if (book.loadError) throw new Error('本机笔记库读取失败，已停止同步');
  const expected = JSON.stringify(book.data), library = JSON.parse(expected) as Library;
  if (library.notes.some(note => note.readOnly)) throw new Error('笔记库含不可读或损坏的文件，已停止同步，避免上传不完整正文');
  const config = normalizeSettings(settings), storage = createStore(config), target = storage.identity();
  const saved = library.sync?.target === target ? library.sync.state : null;
  let state: core.State = saved ? { ...structuredClone(saved), hashCache: {} } : { deviceId: crypto.randomUUID(), deviceName: '轻羽 QingYu', vaultId: null, baseline: null, hashCache: {} };
  const assets = await book.readSyncAssets();
  const files = new SyncFiles(library, assets);
  progress('正在连接 S3…');
  const key = await openKey(storage);
  let conflictCount = 0;
  for (let attempt = 0; attempt < 6; attempt++) {
    progress(attempt ? '远端发生变化，正在重新合并…' : '正在比较笔记库…');
    const deps: core.Dependencies = { space: core.VAULT_SPACE, adapter: files, storage, scope, key, state,
      maxFileBytes: MAX_ASSET_BYTES, concurrency: config.concurrency, history: { horizon: null } };
    const context: core.Context = { persistState: async next => { state = next; deps.state = next; }, getFreshState: () => state,
      setProgress: () => undefined, reportProgressSoon: () => undefined, logInfo: async () => undefined };
    try {
      let result = await core.compare(deps);
      checkHead(result.remote);
      if (!result.remote && state.baseline) throw new Error('远端索引缺失，未将本机笔记视为已删除；请检查前缀或恢复远端');
      // Equal bytes were read on both sides, even if no push/pull was necessary on this device.
      if (result.remote && result.diff.converged.length) {
        state = { ...state, vaultId: result.remote.vaultId, baseline: core.advanceBaselineForPaths(state.baseline, result.remote,
          new Set(result.diff.converged), result.snapshot.emptyFolders, scope), hashCache: {} };
        deps.state = state;
      }
      if (result.diff.conflicts.length) {
        progress('正在合并修改…');
        await core.autoMergeOp(deps, { ...result, diff: { ...result.diff, conflicts: result.diff.conflicts.filter(c => !c.path.startsWith('assets/')) } }, context);
        result = await core.compare(deps);
        checkHead(result.remote);
        if (result.diff.conflicts.length) {
          conflictCount += await preserveConflicts(files, deps, result);
          state = deps.state;
          result = await core.compare(deps);
          checkHead(result.remote);
        }
      }
      if (result.diff.conflicts.length) throw new Error('远端持续变化，请稍后再次同步');
      if (result.diff.remoteChanges.length) {
        progress('正在接收远端笔记…');
        const outcome = await core.pullPathsOp(deps, result, result.diff.remoteChanges.map(change => change.path), context);
        for (const path of outcome.touchedPaths) {
          const value = files.files.get(path), entry = result.remote?.files[path];
          if (value && entry) value.mtime = entry.mtime;
        }
        state.hashCache = {}; deps.state = state;
        result = await core.compare(deps);
        checkHead(result.remote);
      }
      // A changed remote after the pull is compared again rather than pushed over.
      if (result.diff.conflicts.length || result.diff.remoteChanges.length) continue;
      const merged = files.toLibrary(library.activeId);
      const afterAssets = files.toAssets();
      if (result.diff.localChanges.length) {
        progress('正在上传笔记…');
        const head = await core.pushPaths(deps, result, result.diff.localChanges.map(change => change.path));
        state = core.advanceSessionAfterPush(state, result, head, scope);
      }
      progress('正在保存同步结果…');
      if (!await beforeCommit()) throw new Error('当前输入尚未保存，已保留本机内容，请完成输入后再次同步');
      await book.commitSync(merged, { version: 1, target, state: { ...state, hashCache: {} } }, expected, { before: assets, after: afterAssets });
      return { conflicts: conflictCount, notes: merged.notes.length, folders: merged.folders.length, assets: afterAssets.length };
    } catch (error) {
      if (error instanceof core.ConcurrentPushError) continue;
      throw error;
    }
  }
  throw new Error('多台设备持续修改，已保留本机内容，请稍后再次同步');
}
