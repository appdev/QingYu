import { SyncSnapshotChanged, type Folder, type Library, type Notebook } from './notebook';
import * as core from './vendor/mdsync/core';
import { SyncFiles, scope, ownedPath, notePath, metaPath, folderPath, assetPath, assetRelative, MAX_ASSET_BYTES, type AssetPlaceholder, type NoteMeta } from './sync-files';
import { createStore, normalizeSettings, openKey, syncChunkIds, type S3Connection } from './sync-store';

const textEncoder = new TextEncoder();
const AUTO_DOWNLOAD_BYTES = 1_000_000;
const staleAssetRevision = '笔记位置或版本已变化，请重新同步';
const parse = <T>(text: string | null): T => { if (text === null) throw new Error('远端同步文件缺失'); return JSON.parse(text) as T; };
async function copyId(id: string, token: string) { return `conflict-${(await core.sha256Hex(textEncoder.encode(`${id}\n${token}`))).slice(0, 40)}`; }
function checkHead(head: core.Head | null) {
  if (!head) return;
  if (head.version !== 3 || !head.files || Object.keys(head.files).length > 30000 || Object.entries(head.files).some(([path, entry]) =>
    !ownedPath(path) || !/^[a-f0-9]{64}$/.test(entry.hash) || !Number.isFinite(entry.size) || entry.size < 0 || entry.size > (path.startsWith('assets/') ? MAX_ASSET_BYTES : 4 * 1024 * 1024) ||
    !Number.isFinite(entry.mtime) || entry.mtime < 0 || !Number.isFinite(new Date(entry.mtime).getTime()))) throw new Error('远端笔记库协议或文件格式无效，已停止同步');
}

function conflictCopyPath(path: string, hash: string) {
  const relative = assetRelative(path), slash = relative.lastIndexOf('/'), dot = relative.lastIndexOf('.');
  const split = dot > slash + 1 ? dot : relative.length;
  return assetPath(`${relative.slice(0, split)}（冲突-${hash.slice(0, 12)}）${relative.slice(split)}`);
}
async function downloadRemote(deps: core.Dependencies, entry: Pick<core.Entry, 'hash' | 'size'>) {
  const bytes = await core.loadRemoteBytes(deps, entry.hash, entry.size);
  if (!bytes || bytes.length !== entry.size) throw new Error('附件冲突版本未完整下载，已停止同步');
  return bytes;
}
// 远端版本另存为冲突副本。同名副本已存在时，只接受内容相同的那个；占位附件直接比对记录的哈希，不去读字节。
async function keepRemoteCopy(files: SyncFiles, deps: core.Dependencies, path: string, entry: Pick<core.Entry, 'hash' | 'size' | 'mtime'>) {
  const destination = conflictCopyPath(path, entry.hash);
  if (files.files.has(destination)) {
    const existing = files.verifiedAsset(destination)?.hash ?? await core.sha256Hex(new Uint8Array(await files.readBinary(destination)));
    if (existing !== entry.hash) throw new Error('附件冲突副本名称已被占用，未覆盖文件');
    return;
  }
  await files.writeAsset(destination, await downloadRemote(deps, entry), entry.mtime);
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
    if (other && local) await keepRemoteCopy(files, deps, path, other);
    else if (other) await files.writeAsset(path, await downloadRemote(deps, other), other.mtime);
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
        // 分享链接属于原笔记，冲突副本不能共用同一个链接。
        const { share: _share, ...rest } = other;
        const meta: NoteMeta = { ...rest, id: duplicate, conflictOf: id };
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
  deps.state = { ...deps.state, vaultId: remote.vaultId, baseline: base };
  return result.diff.conflicts.length;
}

type SyncBook = Pick<Notebook, 'data' | 'loadError' | 'root' | 'assetRevision' | 'readSyncAssets' | 'readAssetPlaceholders' | 'readAsset' | 'stageAsset' | 'readStagedAsset' | 'releaseAssetStage' | 'commitSync'>;

export async function syncNotebook(book: SyncBook, settings: S3Connection, progress: (message: string) => void = () => undefined, beforeCommit: () => boolean | Promise<boolean> = () => true, resolveKey = openKey): Promise<{
  conflicts: number; notes: number; folders: number; assets: number; notice?: string;
}> {
  if (book.loadError) throw new Error('本机笔记库读取失败，已停止同步');
  const library = structuredClone(book.data);
  if (library.notes.some(note => note.readOnly)) throw new Error('笔记库含不可读或损坏的文件，已停止同步，避免上传不完整正文');
  const config = normalizeSettings(settings), storage = createStore(config), target = storage.identity();
  const cloudBinding = config.cloud && await core.sha256Hex(textEncoder.encode(JSON.stringify([settings.accountId, config.cloud.repository])));
  const previous = library.sync;
  if (cloudBinding && previous?.cloudBinding && previous.cloudBinding !== cloudBinding) throw new Error('account_changed');
  const changedDirectory = !!(cloudBinding && previous && previous.target !== target &&
    (previous.cloudBinding || /\|users\/[0-9a-f-]+\/vault-v2$/.test(previous.target)));
  const canUpload = settings.canUpload !== false && !changedDirectory;
  const saved = library.sync?.target === target ? library.sync.state : null;
  let state: core.State = saved ? structuredClone(saved) : { deviceId: crypto.randomUUID(), deviceName: '轻羽 QingYu', vaultId: null, baseline: null, hashCache: {} };
  const location = { root: book.root, revision: book.assetRevision };
  const [assets, placeholders] = await Promise.all([book.readSyncAssets(), book.readAssetPlaceholders()]).catch(reason => {
    if (reason === staleAssetRevision && book.root === location.root) throw new SyncSnapshotChanged();
    throw reason;
  });
  const savedCount = library.sync?.version === 2 ? library.sync.placeholderCount : 0;
  if (Object.keys(placeholders).length !== savedCount) throw new Error('附件占位索引与同步记录不匹配，已停止同步');
  if (savedCount && !saved) throw new Error('请先使用原 S3 连接下载全部附件，再切换同步目标');
  const batch = crypto.randomUUID();
  let hasStage = false;
  const files = new SyncFiles(library, assets, {
    read: asset => book.readAsset(asset, location),
    stage: bytes => {
      hasStage = true;
      return book.stageAsset(batch, bytes, location);
    },
    readStage: asset => book.readStagedAsset(batch, asset),
  });
  // 占位路径上又出现了本机文件，比如从 Finder 拷进来的同名文件。
  const shadowed = new Map<string, AssetPlaceholder>();
  for (const [path, value] of Object.entries(placeholders)) {
    const key = assetPath(path);
    assetRelative(key);
    const entry = saved?.baseline?.files[key];
    if (!entry || entry.hash !== value.hash || entry.size !== value.size || entry.mtime !== value.mtime) {
      throw new Error('附件占位与同步基线不匹配，已停止同步');
    }
    if (files.files.has(key)) shadowed.set(key, value);
    else files.setPlaceholder(key, value);
  }
  const onDemand = settings.assetDownload === 'on-demand';
  try {
    return await syncFiles();
  } finally {
    if (hasStage) await book.releaseAssetStage(batch).catch(() => undefined);
  }

  async function syncFiles() {
    progress('正在连接 S3…');
    const key = await resolveKey(storage);
    const chunkIds = config.cloud ? syncChunkIds(key) : undefined;
    let conflictCount = 0;
    let originalEntries: Record<string, core.Entry> | undefined;
    for (let attempt = 0; attempt < 6; attempt++) {
      progress(attempt ? '远端发生变化，正在重新合并…' : '正在比较笔记库…');
      const deps: core.Dependencies = { space: core.VAULT_SPACE, adapter: files, storage, scope, key, state, chunkIds,
        maxFileBytes: MAX_ASSET_BYTES, concurrency: config.concurrency, history: { horizon: null } };
      const context: core.Context = { persistState: async next => { state = next; deps.state = next; }, getFreshState: () => state,
        setProgress: () => undefined, reportProgressSoon: () => undefined, logInfo: async () => undefined };
      const compare = async () => {
        // Hash only disk-backed entries can reuse local cache records after adapter writes.
        deps.state.hashCache = Object.fromEntries(Object.entries(deps.state.hashCache).filter(([path]) => {
          const value = files.files.get(path);
          return value && 'original' in value && assetPath(value.original.path) === path;
        }));
        const result = await core.compare(deps);
        checkHead(result.remote);
        if (result.snapshot.skipped.length) {
          if (book.root === location.root && book.assetRevision !== location.revision) throw new SyncSnapshotChanged();
          throw new Error(`无法扫描同步文件：${result.snapshot.skipped[0]!.path}`);
        }
        state.hashCache = result.updatedCache;
        return result;
      };
      try {
        let result = await compare();
        originalEntries ??= result.snapshot.files;
        if (shadowed.size) {
          // 本机文件会作为新版本上传。用户从没见过远端那份，先把它另存为冲突副本，免得被悄悄覆盖。
          for (const [path, entry] of shadowed) {
            if (result.snapshot.files[path]?.hash !== entry.hash && result.remote?.files[path]?.hash === entry.hash) {
              progress('正在保留远端附件…');
              await keepRemoteCopy(files, deps, path, entry);
            }
          }
          shadowed.clear();
          result = await compare();
        }
        if (!result.remote && state.baseline) throw new Error('远端索引缺失，未将本机笔记视为已删除；请检查前缀或恢复远端');
        // Equal bytes were read on both sides, even if no push/pull was necessary on this device.
        if (result.remote && result.diff.converged.length) {
          state = { ...state, vaultId: result.remote.vaultId, baseline: core.advanceBaselineForPaths(state.baseline, result.remote,
            new Set(result.diff.converged), result.snapshot.emptyFolders, scope) };
          deps.state = state;
        }
        if (result.diff.conflicts.length) {
          progress('正在合并修改…');
          await core.autoMergeOp(deps, { ...result, diff: { ...result.diff, conflicts: result.diff.conflicts.filter(c => !c.path.startsWith('assets/')) } }, context);
          result = await compare();
          if (result.diff.conflicts.length) {
            conflictCount += await preserveConflicts(files, deps, result);
            state = deps.state;
            result = await compare();
          }
        }
        if (result.diff.conflicts.length) throw new Error('远端持续变化，请稍后再次同步');
        if (result.diff.remoteChanges.length) {
          progress('正在接收远端笔记…');
          const remote = result.remote;
          if (!remote) throw new Error('缺少远端同步记录');
          // A real local file can follow a rename without downloading its bytes from S3.
          const retainedMoves = new Set(result.diff.moves.flatMap(move => {
            const source = files.files.get(move.from);
            return move.side === 'local' || source && !('placeholder' in source) ? [move.from, move.to] : [];
          }));
          const deferred = new Set(result.diff.remoteChanges.map(({ path }) => path).filter(path =>
            onDemand && path.startsWith('assets/') && !retainedMoves.has(path) && (remote.files[path]?.size ?? 0) >= AUTO_DOWNLOAD_BYTES));
          const pull = result.diff.remoteChanges.map(({ path }) => path).filter(path => !deferred.has(path));
          if (pull.length) {
            const moves = result.diff.moves.filter(({ from, to }) => !deferred.has(from) && !deferred.has(to));
            const outcome = await core.pullPathsOp(deps, { ...result, diff: { ...result.diff, moves } }, pull, context);
            for (const path of outcome.touchedPaths) {
              const value = files.files.get(path), entry = remote.files[path];
              if (value && entry) value.mtime = entry.mtime;
            }
          }
          if (deferred.size) {
            for (const path of deferred) files.setPlaceholder(path, remote.files[path]!);
            state = { ...state, vaultId: remote.vaultId, baseline: core.advanceBaselineForPaths(state.baseline, remote, deferred, result.snapshot.emptyFolders, scope) };
            deps.state = state;
          }
          result = await compare();
        }
        // A changed remote after the pull is compared again rather than pushed over.
        if (result.diff.conflicts.length || result.diff.remoteChanges.length) continue;
        if (!onDemand) {
          for (const [path, value] of files.files) {
            if (!('placeholder' in value)) continue;
            progress('正在下载附件…');
            const bytes = await core.loadRemoteBytes(deps, value.placeholder, value.size);
            if (bytes?.length !== value.size) throw new Error('附件下载不完整，已停止同步');
            await files.writeAsset(path, bytes, value.mtime);
          }
        }
        const merged = files.toLibrary(library.activeId);
        const { sync, count } = await files.assetChanges(originalEntries);
        let published: core.State | undefined;
        if (canUpload && result.diff.localChanges.length) {
          progress('正在上传笔记…');
          const paths = result.diff.localChanges.map(change => change.path);
          const head = await core.pushPaths(deps, result, paths);
          // On a stale local snapshot, acknowledge only exact local versions that reached S3.
          const acknowledged = paths.filter(path => originalEntries![path]?.hash === head.files[path]?.hash);
          if (acknowledged.length) {
            published = { ...state, vaultId: head.vaultId, hashCache: {},
              baseline: core.advanceBaselineForPaths(saved?.baseline ?? null, head, new Set(acknowledged), [], scope) };
          }
          state = core.advanceSessionAfterPush(state, result, head, scope);
        }
        progress('正在保存同步结果…');
        const ready = await beforeCommit();
        const placeholderCount = Object.keys(sync.placeholders).length;
        const checkpoint = { target, state, ...(cloudBinding ? { cloudBinding } : {}) };
        await book.commitSync(merged, placeholderCount ? { ...checkpoint, version: 2, placeholderCount } : { ...checkpoint, version: 1 }, library, sync, location, published, ready);
        const notice = changedDirectory ? '云存储目录已变更，已确认新基线，下次同步可上传' :
          settings.canUpload === false ? '轻羽云存储写入权益已到期，续费后可继续上传' : undefined;
        return { conflicts: conflictCount, notes: merged.notes.length, folders: merged.folders.length, assets: count, notice };
      } catch (error) {
        if (error instanceof core.ConcurrentPushError) continue;
        if (error === staleAssetRevision && book.root === location.root) throw new SyncSnapshotChanged();
        throw error;
      }
    }
    throw new Error('多台设备持续修改，已保留本机内容，请稍后再次同步');
  }
}
