import { describe, expect, it, vi } from 'vitest';
import { Notebook, folderNoteCounts, groupNotes, isShared, legacyDraftKey, libraryKey, noteTitle, visibleNotes, type Library } from '../src/notebook';
import { noteFirstImage, noteMatches, noteSummary, noteText } from '../src/document-text';

it('uses the first line for all notes and falls back to the file name for empty notes', () => {
  const note = { filename: 'B72FA25E-A0C0-48C8-8B25-175B4C44D8E1.md', title: '无标题', source: '1\n\n正文' };
  expect(noteTitle(note)).toBe('1');
  expect(noteTitle({ ...note, source: '# 更新后的标题\n\n正文' })).toBe('更新后的标题');
  expect(noteTitle({ ...note, source: '' })).toBe('无标题');
  expect(noteTitle({ ...note, filename: '自定义名称.md', title: '自定义名称' })).toBe('1');
  expect(noteTitle({ ...note, filename: '无标题.md' })).toBe('1');
});

function storage(initial: Record<string, string> = {}) {
  const values = new Map(Object.entries(initial));
  return { values, getItem: (key: string) => values.get(key) ?? null, setItem: vi.fn((key: string, value: string) => { values.set(key, value); }) };
}

describe('local notebook and legacy migration', () => {
  it('creates one daily note per local day and reopens a renamed note', async () => {
    const memory = storage();
    const book = new Notebook(memory, '# 普通笔记');
    const settings = { todayFolder: '', todayTitle: 'iso' } as const;
    const today = new Date(2026, 10, 16, 14, 5);
    await expect(Promise.all([book.openToday(settings, today), book.openToday(settings, today)])).resolves.toEqual([true, false]);
    const id = book.current.id;
    expect(book.data.folders.find(folder => folder.id === book.current.folderId)?.name).toBe('每日笔记');
    expect(book.current.source).toBe('# 2026-11-16\n');
    await book.renameNote(id, '今日工作');
    const reopened = new Notebook(memory, '');
    await expect(reopened.openToday({ ...settings, todayTitle: 'weekday' }, today)).resolves.toBe(false);
    expect(reopened.current.id).toBe(id);
    expect(reopened.current.title).toBe('今日工作');
    await expect(reopened.openToday(settings, new Date(2026, 10, 17, 0, 1))).resolves.toBe(true);
    expect(reopened.current.source).toBe('# 2026-11-17\n');
    expect(reopened.current.id).not.toBe(id);
  });
  it('opens a previously imported ISO daily note and recreates it after moving it to trash', async () => {
    const book = new Notebook(storage(), '# 普通笔记');
    const imported = await book.createFolder('Journal');
    const data = book.data;
    data.notes.push({ id: 'imported', folderId: imported, title: '2026-11-16', source: '旧日记', createdAt: 1000, updatedAt: 1000 });
    const settings = { todayFolder: '', todayTitle: 'iso' } as const;
    const today = new Date(2026, 10, 16, 14, 5);
    await expect(book.openToday(settings, today)).resolves.toBe(false);
    expect(book.current.id).toBe('imported');
    await book.trashNote('imported', true);
    await expect(book.openToday(settings, today)).resolves.toBe(true);
    expect(book.current.source).toBe('# 2026-11-16\n');
    expect(book.current.id).not.toBe('imported');
  });
  it('migrates the exact legacy source once and preserves the old key as a backup', async () => {
    const source = '---\r\ntitle: 原样\r\n---\r\n# 中文 🌸\r\n\r\n| a | b |\r\n';
    const store = storage({ [legacyDraftKey]: source });
    const book = new Notebook(store, 'starter');
    expect(book.current.source).toBe(source);
    await book.save(source);
    expect(store.values.get(legacyDraftKey)).toBe(source);
    const loaded = new Notebook(store, 'starter');
    expect(loaded.current.source).toBe(source);
    await loaded.save(source);
    expect(store.setItem).toHaveBeenCalledTimes(1);
  });
  it('preserves an intentionally empty legacy draft', async () => {
    const book = new Notebook(storage({ [legacyDraftKey]: '' }), '# starter');
    expect(book.current.source).toBe('');
  });
  it('persists separate documents, folders and selection without overwriting the first note', async () => {
    const store = storage();
    const book = new Notebook(store, '# 第一篇\n\n正文');
    const first = book.current.id;
    await book.save(book.current.source);
    const folder = await book.createFolder('旅行攻略');
    await book.createNote(folder);
    const second = book.current.id;
    await book.save('# 第二篇\n\n旅途 🌱');
    await book.select(first);
    expect(book.current.source).toBe('# 第一篇\n\n正文');
    const reloaded = new Notebook(store, 'starter');
    expect(reloaded.current.id).toBe(first);
    await reloaded.select(second);
    expect(reloaded.current.source).toBe('# 第二篇\n\n旅途 🌱');
    expect(reloaded.current.folderId).toBe(folder);
    expect(reloaded.data.folders.map(f => f.name)).toEqual(['备忘录', '旅行攻略']);
  });
  it('keeps state and persisted content unchanged when saving, selecting or creating exceeds quota', async () => {
    const store = storage();
    const book = new Notebook(store, '# 已保存');
    await book.createNote('default');
    await book.save('# 第二篇');
    const before = JSON.stringify(book.data), disk = store.values.get(libraryKey);
    store.setItem.mockImplementation(() => { throw new DOMException('full', 'QuotaExceededError'); });
    for (const mutation of [() => book.save('# 尚未保存'), () => book.select(book.data.notes[1].id), () => book.createNote('default'), () => book.createFolder('工作')]) {
      await expect(mutation()).rejects.toThrow();
      expect(JSON.stringify(book.data)).toBe(before);
      expect(store.values.get(libraryKey)).toBe(disk);
    }
  });
  it('does not overwrite malformed or unsupported persisted libraries', async () => {
    for (const raw of ['{broken', '{"version":99}', '{"version":1,"folders":[],"notes":[],"activeId":"x"}']) {
      const store = storage({ [libraryKey]: raw });
      const book = new Notebook(store, '# fallback');
      expect(book.loadError).toBe(true);
      await expect(book.save('# edit')).rejects.toThrow('未覆盖');
      expect(store.setItem).not.toHaveBeenCalled();
      expect(store.values.get(libraryKey)).toBe(raw);
    }
  });
  it('rejects invalid references and duplicate ids instead of silently losing notes', async () => {
    const store = storage();
    const seed = new Notebook(store, '# first');
    await seed.save(seed.current.source);
    for (const change of [(x: Library) => { x.activeId = 'missing'; }, (x: Library) => { x.notes[0].folderId = 'missing'; }, (x: Library) => { x.notes.push(x.notes[0]); }, (x: Library) => { x.notes[0].updatedAt = 1e25; }]) {
      const data: Library = JSON.parse(store.values.get(libraryKey)!); change(data);
      expect(new Notebook(storage({ [libraryKey]: JSON.stringify(data) }), '').loadError).toBe(true);
    }
  });
  it('rejects blank or duplicate folders and unknown selection targets', async () => {
    const book = new Notebook(storage(), '');
    await expect(book.createFolder('   ')).rejects.toThrow('名称');
    await expect(book.createFolder('  备忘录  ')).rejects.toThrow('同名');
    await expect(book.select('missing')).rejects.toThrow('不存在');
    await expect(book.createNote('missing')).rejects.toThrow('不存在');
  });
});

describe('note context operations', () => {
  it('renames the first line, finds the new title and duplicates the exact latest source', async () => {
    const store = storage(), book = new Notebook(store, '# 原标题\r\n\r\n未修改正文\r\n');
    const id = book.current.id;
    await book.renameNote(id, '独立名称');
    const source = book.current.source;
    expect(source).toBe('# 独立名称\r\n\r\n未修改正文\r\n');
    expect(visibleNotes(book.data, null, '独立名称').map(note => note.id)).toEqual([id]);
    await book.save(source + '最新内容'); await book.duplicateNote(id);
    expect(book.current.id).not.toBe(id); expect(noteTitle(book.current)).toBe('独立名称');
    expect(book.current.title).toBe('独立名称 Copy');
    expect(book.current.source).toBe(source + '最新内容');
    await book.duplicateNote(id); expect(book.current.title).toBe('独立名称 Copy 2');
    expect(new Notebook(store, '').data).toEqual(book.data);
  });
  it('pins ahead of years and moves/trashes/restores without removing the last note or its bytes', async () => {
    const store = storage(), book = new Notebook(store, '# 唯一笔记\r\n');
    const id = book.current.id, source = book.current.source, folder = await book.createFolder('旅行');
    await book.togglePin(id); expect(groupNotes(visibleNotes(book.data, null, ''))[0].year).toBe(0);
    await book.moveNote(id, folder); await book.trashNote(id);
    expect(visibleNotes(book.data, null, '')).toEqual([]);
    expect(groupNotes(visibleNotes(book.data, null, '', true))[0].year).not.toBe(0);
    expect(book.current.source).toBe(source); expect(book.current.folderId).toBe(folder);
    expect(new Notebook(store, '').loadError).toBe(false);
    await book.trashNote(id, false); expect(visibleNotes(book.data, folder, '')[0].id).toBe(id);
    await book.togglePin(id); expect(book.current.pinned).toBe(false);
    await book.trashNote(id); await book.moveNote(id, 'default'); expect(book.current.trashed).toBe(false);
  });
  it('keeps share state in metadata, validates it and never copies it into a duplicate', async () => {
    const store = storage(), book = new Notebook(store, '# 分享笔记');
    const id = book.current.id, url = 'https://share.example.com/n/abc';
    book.data.notes[0].share = { status: 'shared', url }; await book.save(book.current.source);
    expect(new Notebook(store, '').current.share).toEqual({ status: 'shared', url });
    expect(isShared(book.current)).toBe(true);
    expect(isShared({ share: { status: 'unshared', url } })).toBe(false);
    expect(isShared({ share: { status: 'shared', url }, trashed: true })).toBe(false);
    expect(isShared({})).toBe(false);
    await expect(book.setShare(id, { status: 'shared', url: 'ftp://share.example.com/n/abc' })).rejects.toThrow('分享链接无效');
    await book.setShare(id, { status: 'unshared' }); expect(new Notebook(store, '').current.share).toEqual({ status: 'unshared' });
    await book.setShare(id, { status: 'shared', url });
    await book.duplicateNote(id);
    expect(book.current.id).not.toBe(id); expect(book.current.share).toBeUndefined();
    for (const share of [null, 'shared', { status: 'public' }, { status: 'shared', url: 1 }, { status: 'shared', url: 'javascript:alert(1)' }, { status: 'shared', url: 'not a url' }]) {
      const invalid = structuredClone(book.data); (invalid.notes[0] as { share?: unknown }).share = share;
      expect(new Notebook(storage({ [libraryKey]: JSON.stringify(invalid) }), '').loadError).toBe(true);
    }
  });
  it('rejects invalid targets, names and metadata, and preserves all state on storage failure', async () => {
    const store = storage(), book = new Notebook(store, '# 内容'); await book.save(book.current.source);
    await expect(book.renameNote(book.current.id, '  ')).rejects.toThrow();
    await expect(book.renameNote(book.current.id, 'a\nb')).rejects.toThrow();
    await expect(book.moveNote(book.current.id, 'missing')).rejects.toThrow();
    await expect(book.duplicateNote('missing')).rejects.toThrow();
    for (const patch of [{ title: '' }, { pinned: 'yes' }, { trashed: 1 }]) {
      const invalid = structuredClone(book.data); Object.assign(invalid.notes[0], patch);
      expect(new Notebook(storage({ [libraryKey]: JSON.stringify(invalid) }), '').loadError).toBe(true);
    }
    const before = JSON.stringify(book.data);
    store.setItem.mockImplementation(() => { throw new Error('disk full'); });
    for (const run of [() => book.renameNote(book.current.id, '新名称'), () => book.togglePin(book.current.id), () => book.moveNote(book.current.id, 'default'), () => book.trashNote(book.current.id), () => book.duplicateNote(book.current.id)]) {
      await expect(run()).rejects.toThrow(); expect(JSON.stringify(book.data)).toBe(before); expect(store.values.get(libraryKey)).toBe(before);
    }
  });
});

describe('sidebar document information', () => {
  it('keeps pinned notes first and sorts equal titles by latest edit in either direction', () => {
    const book = new Notebook(storage(), '# 笔记');
    const first = book.current;
    book.data.notes = [
      { ...first, id: 'older', title: '旧文件名', source: '# 项目 2', updatedAt: 100 },
      { ...first, id: 'pinned', source: '# 项目 10', updatedAt: 100, pinned: true },
      { ...first, id: 'recent', source: '# 项目 2', updatedAt: 200 },
      { ...first, id: 'other', source: '# 项目 3', updatedAt: 300 },
    ];
    const sorted = (reverse: boolean) => visibleNotes(book.data, null, '', false, { key: 'title', reverse, grouped: true }).map(note => note.id);
    expect(sorted(false)).toEqual(['pinned', 'recent', 'older', 'other']);
    expect(sorted(true)).toEqual(['pinned', 'other', 'recent', 'older']);
  });

  it('groups recent notes by day and older notes by month, keeping pinned notes together', () => {
    vi.useFakeTimers();
    try {
      vi.setSystemTime(new Date(2026, 10, 11));
      const first = new Notebook(storage(), '# 笔记').current;
      const notes = [
        { ...first, id: 'april', updatedAt: new Date(2026, 3, 2).getTime() },
        { ...first, id: 'february-a', updatedAt: new Date(2026, 1, 27).getTime() },
        { ...first, id: 'february-b', updatedAt: new Date(2026, 1, 8).getTime() },
        { ...first, id: 'last-november', updatedAt: new Date(2025, 10, 26).getTime() },
        { ...first, id: 'last-october', updatedAt: new Date(2025, 9, 3).getTime() },
        { ...first, id: 'pinned-old', pinned: true, updatedAt: new Date(2025, 1, 1).getTime() },
        { ...first, id: 'pinned-new', pinned: true, updatedAt: new Date(2026, 10, 11).getTime() },
        { ...first, id: 'future', updatedAt: new Date(2027, 0, 1).getTime() },
      ];
      expect(groupNotes(notes).map(g => [g.label, g.notes.map(n => n.id)])).toEqual([
        ['置顶', ['pinned-new', 'pinned-old']], ['今天', ['future']], ['4月', ['april']],
        ['2月', ['february-a', 'february-b']], ['2025/11', ['last-november']], ['2025/10', ['last-october']],
      ]);
      vi.setSystemTime(new Date(2027, 0, 1));
      expect(groupNotes(notes).map(g => g.label)).toEqual(['置顶', '今天', '2026/4', '2026/2', '2025/11', '2025/10']);
    } finally { vi.useRealTimers(); }
  });
  it('uses the shared frontmatter rules for titles and extracts a plain summary', async () => {
    expect(noteText('---\ntitle: hidden\n---\n# **中文标题**\n\n正文 [链接](https://example.com) 与 `code`')).toEqual({ title: '中文标题', summary: '正文 链接 与 code' });
    expect(noteText('')).toEqual({ title: '未命名', summary: '无更多文本' });
    expect(noteText('# 图片\n\n![照片\\[一\\]|cover](/cover.webp)')).toEqual({ title: '图片', summary: '照片[一]' });
    expect(noteText('# 图片\n\n正文 ![行内图|small](/inline.webp)')).toEqual({ title: '图片', summary: '正文 行内图|small' });
    expect(noteText('# 只有标题')).toEqual({ title: '只有标题', summary: '无更多文本' });
  });
  it('filters title/body case-insensitively within a folder and groups by modification year', async () => {
    const book = new Notebook(storage(), '# 2021');
    const first = book.current;
    const library: Library = { ...book.data, folders: [...book.data.folders, { id: 'travel', name: '旅行攻略' }], notes: [
      { ...first, id: 'a', source: '# CodeMirror\n\n检索正文', updatedAt: new Date(2021, 3, 29).getTime() },
      { ...first, id: 'b', folderId: 'travel', source: '# 旅行\n\nCODEMIRROR', updatedAt: new Date(2020, 11, 6).getTime() },
      { ...first, id: 'c', source: '# 旧笔记', updatedAt: new Date(2019, 7, 30).getTime() },
    ] };
    expect(visibleNotes(library, null, ' codemirror ').map(n => n.id)).toEqual(['a', 'b']);
    expect(visibleNotes(library, 'default', '检索正文').map(n => n.id)).toEqual(['a']);
    expect(groupNotes(library.notes).map(g => [g.year, g.notes.map(n => n.id)])).toEqual([[2021, ['a']], [2020, ['b']], [2019, ['c']]]);
    expect(groupNotes([])).toEqual([]);
  });
});

it('counts folder notes in one pass with the same rules as the folder list', () => {
  const note = (id: string, folderId: string, trashed = false) => ({ id, folderId, source: id, createdAt: 1, updatedAt: 1, trashed });
  const library: Library = { version: 1, activeId: 'a', folders: [
    { id: 'root', name: '备忘录', path: '' }, { id: 'work', name: '工作', path: '工作' },
    { id: 'deep', name: '项目', path: '工作/项目' }, { id: 'like', name: '工作簿', path: '工作簿' }, { id: 'named', name: '旧版' },
  ], notes: [note('a', 'root'), note('b', 'work'), note('c', 'deep'), note('d', 'deep', true), note('e', 'like'), note('f', 'named')] };
  const counts = folderNoteCounts(library);
  for (const folder of library.folders) expect(counts.get(folder.id)).toBe(visibleNotes(library, folder.id, '').length);
  expect(counts.get('work')).toBe(2);
});

it('reuses parsed note text until the source changes, even on the same object', () => {
  const note = { source: '# 标题\n摘要\n![图](assets/a.png)' };
  expect([noteTitle(note), noteSummary(note), noteFirstImage(note)]).toEqual(['标题', '摘要', 'assets/a.png']);
  note.source = '新标题\n新的摘要';
  expect([noteTitle(note), noteSummary(note), noteFirstImage(note)]).toEqual(['新标题', '新的摘要', undefined]);
  expect(noteTitle({ source: '' })).toBe(noteText('').title);
});

it('searches the current source after a note changes in place', () => {
  const note = { id: 'n', folderId: 'default', source: '# Alpha\n旧内容', createdAt: 1, updatedAt: 1 };
  const library: Library = { version: 1, activeId: 'n', folders: [{ id: 'default', name: '备忘录' }], notes: [note] };
  expect(visibleNotes(library, null, '旧内容')).toHaveLength(1);
  note.source = '# Alpha\n新内容';
  expect(visibleNotes(library, null, '旧内容')).toHaveLength(0);
  expect(visibleNotes(library, null, '新内容')).toHaveLength(1);
  expect(noteMatches(note, 'alpha')).toBe(true);
});
