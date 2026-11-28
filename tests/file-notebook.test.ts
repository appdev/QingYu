import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { Notebook, NoteSaveConflict, noteTitle, type Library } from '../src/notebook';
import { SyncFiles } from '../src/sync-files';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
const diskSeed: Library = { version: 1, activeId: 'disk', folders: [{ id: 'default', name: '备忘录', path: '' }], notes: [{ id: 'disk', folderId: 'default', source: '# 来自真实文件\r\n', title: '磁盘名称', filename: '磁盘名称.md', path: '磁盘名称.md', createdAt: 1, updatedAt: 1 }] };
let disk: Library, revision: number;
beforeEach(() => {
  vi.mocked(invoke).mockReset(); disk = structuredClone(diskSeed); revision = 1;
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === 'library_commit') {
      const request = args as { next: Library; revision: number };
      expect(request.revision).toBe(revision); disk = structuredClone(request.next); revision++;
    } else if (command === 'library_save_note') {
      const request = args as { noteId: string; expectedSource: string; source: string; revision: number };
      const note = disk.notes.find(note => note.id === request.noteId)!;
      if (note.source !== request.expectedSource) throw new Error('笔记正文已更新，未覆盖');
      const stale = request.revision !== revision;
      note.source = request.source; revision++;
      // 窗口里的版本还是最新的，就只回传这一篇；别处改过才回整份快照。
      if (!stale) return { root: '/test/documents/MiaoYan Editor', revision, note: structuredClone(note) };
    } else if (command === 'library_select') {
      const request = args as { noteId: string; revision: number };
      expect(request.revision).toBe(revision); disk.activeId = request.noteId; return null;
    }
    return { library: structuredClone(disk), root: '/test/documents/MiaoYan Editor', revision };
  });
});
function fixture() { const storage = { getItem: () => null, setItem: vi.fn() }; return { storage, book: new Notebook(storage, '# 旧草稿') }; }

it('reads the native save conflict code independently of the error copy', async () => {
  const { book } = fixture();
  await book.openFiles();
  vi.mocked(invoke).mockRejectedValueOnce({ code: 'conflict', message: 'Changed on disk' });
  await expect(book.save('unsaved', book.current.id, book.current.source)).rejects.toBeInstanceOf(NoteSaveConflict);
  vi.mocked(invoke).mockRejectedValueOnce({ code: 'failure', message: 'Read-only volume' });
  await expect(book.save('unsaved', book.current.id, book.current.source)).rejects.toThrow('Read-only volume');
});

it('passes the reserved destination IDs to the native destination picker', async () => {
  const { book } = fixture();
  disk.folders.push({ id: 'reserved', name: 'Jóts', path: 'Jóts' },
    { id: 'reserved-child', name: 'Child', path: 'Jóts/Child' },
    { id: 'from', name: 'Source', path: 'Source' });
  await book.openFiles();
  await book.folderAction('move', 'from');
  expect(invoke).toHaveBeenLastCalledWith('library_folder_action', expect.objectContaining({
    action: 'move', folderId: 'from', excludedMoveDestinations: ['reserved', 'reserved-child'],
  }));
});

describe('file-backed Notebook contract', () => {
  it('switches notes in memory and writes the active note to disk once the user settles', async () => {
    vi.useFakeTimers();
    try {
      const { book } = fixture();
      disk.notes.push({ ...diskSeed.notes[0], id: 'second', title: '第二篇', filename: '第二篇.md', path: '第二篇.md' }, { ...diskSeed.notes[0], id: 'third', title: '第三篇', filename: '第三篇.md', path: '第三篇.md' });
      await book.openFiles();
      vi.mocked(invoke).mockClear();
      // 连续切换不碰磁盘，当前笔记立即变化。
      expect(await book.select('second')).toBe(true);
      expect(await book.select('third')).toBe(true);
      expect(book.current.id).toBe('third');
      expect(invoke).not.toHaveBeenCalled();
      // 期间别的保存读回旧快照，也不会把当前笔记切回去。
      await book.save('# 改动\n', 'third');
      expect(book.current.id).toBe('third');
      await vi.advanceTimersByTimeAsync(1500);
      expect(vi.mocked(invoke).mock.calls.map(([command]) => command).filter(command => command !== 'library_save_note')).toEqual(['library_select']);
      expect(disk.activeId).toBe('third');
      // 写过之后不再重复写。
      vi.mocked(invoke).mockClear();
      expect(await book.flushActive()).toBe(true);
      expect(invoke).not.toHaveBeenCalled();
    } finally { vi.useRealTimers(); }
  });

  it('follows the first line for new notes and defers the disk name until confirmation', async () => {
    const { book } = fixture();
    await book.openFiles();
    await book.createNote('default');
    const id = book.current.id;
    expect(book.current.followTitle).toBeUndefined();
    await book.save('# 磁盘名称\n', id);
    expect(noteTitle(book.current)).toBe('磁盘名称');
    expect(invoke).toHaveBeenLastCalledWith('library_save_note', expect.objectContaining({ noteId: id }));
    await book.save('# 磁盘名称\n', id, undefined, true);
    expect(book.current).toMatchObject({ id, title: '磁盘名称 2', followTitle: '磁盘名称' });
    expect(new SyncFiles(book.data).toLibrary(id).notes.find(note => note.id === id)).toMatchObject({
      title: '磁盘名称 2', followTitle: '磁盘名称', source: '# 磁盘名称\n',
    });
    await book.save('', id, undefined, true);
    expect(book.current.title).toBe('磁盘名称 2');
    await book.renameNote(id, '手动标题');
    expect(noteTitle(book.current)).toBe('手动标题');
    await book.save('# 再次修改\n', id, undefined, true);
    expect(book.current.title).toBe('再次修改');
  });

  it('uses an imported note’s first line immediately and rewrites its links on confirmed rename', async () => {
    const { book } = fixture();
    disk.notes.push({ id: 'links', folderId: 'default', source: '[[磁盘名称#段落|有别名]]\n[文件](磁盘名称.md#part)\n`[[磁盘名称]]`', path: 'links.md', title: 'links', createdAt: 1, updatedAt: 1 });
    await book.openFiles();
    expect(noteTitle(book.current)).toBe('来自真实文件');
    expect(book.current.title).toBe('磁盘名称');
    await book.save(book.current.source, 'disk', undefined, true);
    expect(book.current).toMatchObject({ id: 'disk', title: '来自真实文件', followTitle: '来自真实文件', source: '# 来自真实文件\r\n' });
    expect(book.data.notes[1].source).toBe('[[备忘录/来自真实文件#段落|有别名]]\n[文件](%E6%9D%A5%E8%87%AA%E7%9C%9F%E5%AE%9E%E6%96%87%E4%BB%B6.md#part)\n`[[磁盘名称]]`');
    expect(invoke).toHaveBeenLastCalledWith('library_commit', expect.objectContaining({ intent: 'title-edit' }));
  });
  it('reads attachment bytes from the captured disk revision through either IPC encoding', async () => {
    const { book } = fixture();
    await book.openFiles();
    const location = { root: book.root, revision: book.assetRevision };
    const asset = { path: '.attachments/test.pdf', size: 3, modified: 1 };
    const bytes = Uint8Array.of(0, 255, 128);
    vi.mocked(invoke).mockResolvedValueOnce([asset]);
    expect(await book.readSyncAssets()).toEqual([asset]);
    expect(invoke).toHaveBeenLastCalledWith('library_sync_assets', location);
    for (const reply of [bytes.buffer, [...bytes]]) {
      vi.mocked(invoke).mockResolvedValueOnce(reply);
      expect(await book.readAsset(asset, location)).toEqual(bytes);
      expect(invoke).toHaveBeenLastCalledWith('library_read_asset', { ...location, asset });
    }
    const large = new Uint8Array(3 * 8192 + 2).fill(255);
    const staged = { path: 'stage', hash: 'hash' };
    vi.mocked(invoke).mockResolvedValueOnce(staged);
    expect(await book.stageAsset('batch', large, location)).toEqual(staged);
    expect(invoke).toHaveBeenLastCalledWith('library_stage_asset', {
      ...location, batch: 'batch', encoded: Buffer.from(large).toString('base64'),
    });
  });

  it('keeps local selection in a note window across shared edits, links, and external deletion', async () => {
    const other = { ...disk.notes[0], id: 'other', source: '另一篇正文' };
    disk.notes.push(other);
    const { book, storage } = fixture();
    const sharedInvoke = vi.mocked(invoke).getMockImplementation()!;
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (command === 'workspace_note_snapshot') {
        const snapshot = await sharedInvoke(command, args) as { library: Library };
        snapshot.library.activeId = (args as { noteId?: string } | undefined)?.noteId ?? 'other';
        return snapshot;
      }
      if (command === 'library_reconcile') {
        return { snapshot: await sharedInvoke(command, args), backups: [] };
      }
      return sharedInvoke(command, args);
    });
    await book.attachNoteWindow();
    expect(book.current.id).toBe('other');
    await book.save('独立窗口正文');
    expect(disk.activeId).toBe('disk');
    expect(book.current).toMatchObject({ id: 'other', source: '独立窗口正文' });
    await book.select('disk');
    await book.select('other');
    expect(invoke).toHaveBeenLastCalledWith('workspace_note_snapshot', { noteId: 'other' });
    disk.notes = [disk.notes[0]]; revision++;
    await book.reconcile(book.root!, { noteId: 'other', source: '待保存正文' });
    expect(invoke).toHaveBeenLastCalledWith('library_reconcile', expect.objectContaining({
      buffer: { noteId: 'other', source: '待保存正文', expectedSource: '独立窗口正文' },
    }));
    expect(book.current).toBeUndefined();
    expect(storage.setItem).not.toHaveBeenCalled();
  });

  it('preserves notes and blocks saves after an inaccessible startup, then retries the disk on reload', async () => {
    const { book, storage } = fixture(), before = structuredClone(book.data);
    vi.mocked(invoke).mockRejectedValueOnce('读取笔记目录超时');
    await expect(book.openFiles()).rejects.toBe('读取笔记目录超时');
    expect(book.loadError).toBe(true); expect(book.data).toEqual(before); expect(book.root).toBeUndefined();
    await expect(book.save('不能覆盖')).rejects.toThrow('笔记库读取失败');
    expect(storage.setItem).not.toHaveBeenCalled();
    await book.reload();
    expect(invoke).toHaveBeenLastCalledWith('library_open', { seed: null });
    expect(book.loadError).toBe(false); expect(book.current.id).toBe('disk');
  });
  it('uses disk source on open and never writes the localStorage backup after migration', async () => {
    const { book, storage } = fixture(); await book.openFiles();
    expect(book.current.source).toBe('# 来自真实文件\r\n');
    await book.save('最新正文'); await book.renameNote('disk', '新文件名');
    expect(disk.notes[0].source).toBe('新文件名'); expect(disk.notes[0].title).toBe('新文件名');
    expect(storage.setItem).not.toHaveBeenCalled();
  });
  it('serializes save before rename and binds delayed saves to their original note', async () => {
    const { book } = fixture(); await book.openFiles();
    await Promise.all([book.save('刚输入的内容', 'disk'), book.renameNote('disk', '新名称'), book.createNote('default')]);
    expect(disk.notes.find(note => note.id === 'disk')).toMatchObject({ source: '新名称', title: '新名称' });
    const active = book.current.id;
    await book.save('稍后提交到原文档', 'disk'); expect(book.current.id).toBe(active); expect(book.current.source).toBe('');
  });
  it('sends only the edited body and its expected source, preserving another note updated on disk', async () => {
    const { book, storage } = fixture(); await book.openFiles();
    const other = { ...disk.notes[0], id: 'other', path: '另一份.md', source: '其他窗口写入' };
    disk.notes.push(other); revision++;
    await book.save('当前窗口正文');
    expect(invoke).toHaveBeenLastCalledWith('library_save_note', {
      root: '/test/documents/MiaoYan Editor', noteId: 'disk', expectedSource: diskSeed.notes[0].source, source: '当前窗口正文', revision: 1,
    });
    expect(book.current).toMatchObject({ id: 'disk', source: '当前窗口正文' });
    expect(book.data.notes.find(note => note.id === 'other')).toEqual(other);
    disk.notes[0].source = '同一篇的另一版本';
    await expect(book.save('当前窗口仍待处理')).rejects.toThrow('正文已更新');
    expect(book.current.source).toBe('当前窗口正文');
    expect(disk.notes[0].source).toBe('同一篇的另一版本');
    expect(storage.setItem).not.toHaveBeenCalled();
  });
  it('keeps the complete in-memory state unchanged when a disk rename is rejected', async () => {
    const { book, storage } = fixture(); await book.openFiles(); const before = JSON.stringify(book.data);
    vi.mocked(invoke).mockRejectedValueOnce('该文件夹中已有同名笔记，未覆盖');
    await expect(book.renameNote('disk', '已有文件')).rejects.toContain('同名');
    expect(JSON.stringify(book.data)).toBe(before); expect(storage.setItem).not.toHaveBeenCalled();
  });
  it('supports empty on-disk libraries without fabricating a replacement note', async () => {
    disk.notes = []; disk.activeId = ''; const { book } = fixture(); await book.openFiles();
    expect(book.data.notes).toEqual([]); await book.save(''); expect(disk.notes).toEqual([]);
    await book.createNote('default'); expect(disk.notes).toHaveLength(1);
  });
  it('ignores stale roots and adopts a disk refresh without writing old localStorage or body back', async () => {
    const { book, storage } = fixture(); await book.openFiles();
    expect(await book.reconcile('/previous-library')).toEqual({ changed: false, backups: [] });
    vi.mocked(invoke).mockResolvedValueOnce({ snapshot: { library: { ...disk, notes: [{ ...disk.notes[0], source: 'external body' }] }, root: book.root, revision: 2 }, backups: ['.miaoyan-conflicts/local.md'] });
    expect(await book.reconcile(book.root!, { noteId: 'disk', source: 'local buffer' })).toEqual({ changed: true, backups: ['.miaoyan-conflicts/local.md'] });
    expect(book.current.source).toBe('external body'); expect(storage.setItem).not.toHaveBeenCalled();
  });
  it('cancels a delayed save captured before an external snapshot was adopted', async () => {
    const { book } = fixture(); await book.openFiles();
    vi.mocked(invoke).mockResolvedValueOnce({ snapshot: { library: { ...disk, notes: [{ ...disk.notes[0], source: 'external body' }] }, root: book.root, revision: 2 }, backups: [] });
    const refresh = book.reconcile(book.root!);
    const staleSave = book.save('old buffered body', 'disk');
    await refresh;
    await expect(staleSave).rejects.toThrow('旧的延迟保存已取消');
    expect(book.current.source).toBe('external body');
  });
});
