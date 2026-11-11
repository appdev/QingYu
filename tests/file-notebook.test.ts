import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { Notebook, type Library } from '../src/notebook';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
const diskSeed: Library = { version: 1, activeId: 'disk', folders: [{ id: 'default', name: '备忘录', path: '' }], notes: [{ id: 'disk', folderId: 'default', source: '# 来自真实文件\r\n', title: '磁盘名称', filename: '磁盘名称.md', path: '磁盘名称.md', createdAt: 1, updatedAt: 1 }] };
let disk: Library, revision: number;
beforeEach(() => {
  vi.mocked(invoke).mockReset(); disk = structuredClone(diskSeed); revision = 1;
  vi.mocked(invoke).mockImplementation(async (command, args) => {
    if (command === 'library_commit') {
      const request = args as { next: Library; revision: number };
      expect(request.revision).toBe(revision); disk = structuredClone(request.next); revision++;
    }
    return { library: structuredClone(disk), root: '/test/documents/MiaoYan Editor', revision };
  });
});
function fixture() { const storage = { getItem: () => null, setItem: vi.fn() }; return { storage, book: new Notebook(storage, '# 旧草稿') }; }

describe('file-backed Notebook contract', () => {
  it('uses disk source on open and never writes the localStorage backup after migration', async () => {
    const { book, storage } = fixture(); await book.openFiles();
    expect(book.current.source).toBe('# 来自真实文件\r\n');
    await book.save('最新正文'); await book.renameNote('disk', '新文件名');
    expect(disk.notes[0].source).toBe('最新正文'); expect(disk.notes[0].title).toBe('新文件名');
    expect(storage.setItem).not.toHaveBeenCalled();
  });
  it('serializes save before rename and binds delayed saves to their original note', async () => {
    const { book } = fixture(); await book.openFiles();
    await Promise.all([book.save('刚输入的内容', 'disk'), book.renameNote('disk', '新名称'), book.createNote('default')]);
    expect(disk.notes.find(note => note.id === 'disk')).toMatchObject({ source: '刚输入的内容', title: '新名称' });
    const active = book.current.id;
    await book.save('稍后提交到原文档', 'disk'); expect(book.current.id).toBe(active); expect(book.current.source).toBe('');
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
