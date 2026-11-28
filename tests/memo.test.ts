import { expect, it, vi } from 'vitest';
import { allMemos, findMemoFolder, groupMemosByDay, hasOpenTask, memoFileTitle, memoNotes, memoTagCounts, memoTags, regularNotes, renderMemo, toggleTask } from '../src/memo';
import { Notebook, NoteSaveConflict, type Library, type Note } from '../src/notebook';

const makeNote = (id: string, source: string, date: Date, extra: Partial<Note> = {}): Note => ({
  id, folderId: 'memos', source, createdAt: date.getTime(), updatedAt: date.getTime(), ...extra,
});
const base: Library = {
  version: 1, folders: [{ id: 'default', name: '备忘录' }, { id: 'nested', name: '随记', path: 'Nested/随记' },
    { id: 'memos', name: 'JÓTS', path: 'JÓTS' }, { id: 'child', name: 'Notes', path: 'JÓTS/Notes' }],
  notes: [], activeId: '',
};

it('formats a portable local timestamp and only recognizes root folders', () => {
  expect(memoFileTitle(new Date(2026, 0, 2, 3, 4, 5))).toBe('2026-01-02 03-04-05');
  expect(findMemoFolder(base)?.id).toBe('memos');
  expect(findMemoFolder({ ...base, folders: base.folders.slice(0, 2) })).toBeUndefined();
});

it('reserves the memo names at the root without reserving them in ordinary subfolders', async () => {
  const book = new Notebook({ getItem: () => null, setItem() {} }, '# Ordinary');
  const parent = await book.createFolder('Projects');
  const child = await book.createFolder('Jots', parent);
  for (const name of ['随记', ' jÓTS ']) {
    await expect(book.createFolder(name)).rejects.toThrow('此名称已保留给随记使用');
    await expect(book.folderAction('rename', parent, name)).rejects.toThrow('此名称已保留给随记使用');
  }
  await book.folderAction('rename', child, '随记');
  await expect(book.folderAction('move-to-folder', child, 'default')).rejects.toThrow('此名称已保留给随记使用');
  await book.createMemo('allowed for Jots');
  const memos = findMemoFolder(book.data)!;
  await expect(book.folderAction('move-to-folder', parent, memos.id)).rejects.toThrow('不能把文件夹放到随记目录中');
  expect(book.data.folders.find(folder => folder.id === child)?.path).toBe('Projects/随记');
});

it('extracts only valid Markdown prose tags and renders them safely', () => {
  const max = 'a'.repeat(32);
  const source = [
    '# Markdown title', '想法：#中文标签 #Ideas #ideas #a-b #project/child/ #1 #2024 a#b',
    '\u0060#code\u0060 https://example.com/#url [a](#anchor) [#link](https://example.com)',
    '\u0060\u0060\u0060', '#fenced', '\u0060\u0060\u0060', '    #indented', '#' + max + ' #' + max + 'a',
    '<img src=x onerror=alert(1)>',
  ].join('\n');
  expect(memoTags(source)).toEqual(['中文标签', 'Ideas', 'a-b', 'project/child', max]);
  const html = renderMemo(makeNote('sample', source, new Date()));
  expect(html).toContain('data-tag="中文标签"');
  expect(html).toContain('&lt;img');
  expect(html).not.toContain('<img ');
});

it('toggles the displayed tasks at the source offset and leaves fenced examples alone', () => {
  const source = '- [ ] first\r\n\r\n\u0060\u0060\u0060md\r\n- [ ] example\r\n\u0060\u0060\u0060\r\n\r\n> - [X] last';
  expect(hasOpenTask(source)).toBe(true);
  expect(toggleTask(source, 0)).toBe(source.replace('[ ] first', '[x] first'));
  expect(toggleTask(source, 1)).toBe(source.replace('[X] last', '[ ] last'));
  expect(toggleTask(source, 2)).toBe(source);
  expect(hasOpenTask(toggleTask(source, 0))).toBe(false);
});

it('combines case-insensitive search and filters, sorts by creation and keeps trashed jots available to the ordinary trash', () => {
  const first = new Date(2026, 10, 26, 20), second = new Date(2026, 10, 27, 9);
  const library = { ...base, notes: [
    makeNote('old', 'Meeting #IDEAS\n\n- [ ] send', first, { updatedAt: second.getTime() + 10 }),
    makeNote('new', 'meeting #ideas\n\n- [x] done', second),
    makeNote('pin', '#Other', first, { pinned: true }),
    makeNote('trashed', '#ideas', second, { trashed: true }),
    makeNote('childNote', '#ideas', second, { folderId: 'child' }),
  ] };
  expect(memoNotes(library, allMemos, '').map(note => note.id)).toEqual(['pin', 'new', 'old']);
  expect(memoNotes(library, { kind: 'tag', tag: 'Ideas' }, 'MEETING').map(note => note.id)).toEqual(['new', 'old']);
  expect(memoNotes(library, { kind: 'todo' }, 'meeting').map(note => note.id)).toEqual(['old']);
  expect(memoTagCounts(library)).toEqual([['ideas', 2], ['Other', 1]]);
  expect(regularNotes(library).map(note => note.id)).toEqual(['trashed', 'childNote']);
});

it('groups pinned entries, local calendar days and dates from another year', () => {
  const now = new Date(2027, 0, 1, 0, 1);
  const notes = [
    makeNote('pin', '', new Date(2025, 0, 1), { pinned: true }),
    makeNote('today', '', now), makeNote('yesterday', '', new Date(2026, 11, 31, 23, 59)),
    makeNote('lastYear', '', new Date(2026, 11, 30)),
  ];
  expect(groupMemosByDay(notes, now, 'zh-CN').map(group => [group.key, group.label])).toEqual([
    ['pinned', '置顶'], ['2027-01-01', '今天'], ['2026-12-31', '昨天'], ['2026-12-30', '2026年12月30日'],
  ]);
  expect(groupMemosByDay(notes, now, 'en').map(group => [group.key, group.label])).toEqual([
    ['pinned', 'Pinned'], ['2027-01-01', 'Today'], ['2026-12-31', 'Yesterday'], ['2026-12-30', 'December 30, 2026'],
  ]);
});

it('creates the memo folder on demand, keeps the current note, and restores a trashed jot', async () => {
  const values = new Map<string, string>();
  const storage = { getItem: (key: string) => values.get(key) ?? null, setItem: (key: string, value: string) => { values.set(key, value); } };
  const book = new Notebook(storage, '# Ordinary');
  const current = book.data.activeId, now = new Date(2026, 0, 2, 3, 4, 5);
  expect(findMemoFolder(book.data)).toBeUndefined();
  const [id, other] = await Promise.all([book.createMemo('hello #ideas', now), book.createMemo('second', now)]);
  expect(book.data.activeId).toBe(current);
  expect(book.data.notes.find(note => note.id === id)).toMatchObject({ title: '2026-01-02 03-04-05', source: 'hello #ideas' });
  expect(book.data.notes.find(note => note.id === other)?.title).toBe('2026-01-02 03-04-05 1');
  await book.save('updated', id, 'hello #ideas');
  await expect(book.save('stale change', id, 'hello #ideas')).rejects.toBeInstanceOf(NoteSaveConflict);
  expect(book.data.notes.find(note => note.id === id)?.title).toBe('2026-01-02 03-04-05');
  await book.trashNote(id);
  expect(memoNotes(book.data, allMemos, '').map(note => note.id)).toEqual([other]);
  await book.trashNote(id, false);
  expect(memoNotes(new Notebook(storage, '').data, allMemos, '').map(note => note.id)).toContain(id);
  book.external = true;
  await expect(book.createMemo('external')).rejects.toThrow();
});

it('uses the winning folder if another browser creates it first', async () => {
  const list = (notes: Note[], folders: Library['folders']) => ({
    folders, notes, version: 'library-version', versions: {},
  });
  const storage = { getItem: () => null, setItem: vi.fn() };
  const book = new Notebook(storage, '# Initial');
  const ordinary = book.current, notes = [ordinary], folders = base.folders.slice(0, 1);
  const updatedFolders = [...folders, base.folders[2]];
  const fetch = vi.fn()
    .mockResolvedValueOnce(Response.json(list(notes, folders)))
    .mockResolvedValueOnce(Response.json({ error: { message: '已有同名文件夹' } }, { status: 409 }))
    .mockResolvedValueOnce(Response.json(list(notes, updatedFolders)))
    .mockImplementationOnce(async (_url: string, request: RequestInit) => {
      const command = JSON.parse(String(request.body));
      expect(command).toMatchObject({ kind: 'createNote', folderId: 'memos', source: 'hello' });
      return Response.json(list([...notes, makeNote(command.id, 'hello', new Date())], updatedFolders));
    });
  vi.stubGlobal('fetch', fetch);
  try {
    await book.openServer();
    const id = await book.createMemo('hello');
    expect(memoNotes(book.data, allMemos, '').map(note => note.id)).toEqual([id]);
    expect(book.data.activeId).toBe(ordinary.id);
  } finally { vi.unstubAllGlobals(); }
});

it('rebases memo attachments on conversion and confirms the filename without rewriting the first line', async () => {
  const values = new Map<string, string>();
  const book = new Notebook({ getItem: key => values.get(key) ?? null, setItem: (key, value) => { values.set(key, value); } }, '# Ordinary');
  const original = '**A thought**\n\n![Photo](.attachments/example/photo.webp)\n\n\u0060![code](example.webp)\u0060';
  const id = await book.createMemo(original);
  book.data.notes.find(note => note.id === id)!.path = '随记/2026-11-27 10-42-15.md';
  const target = await book.createFolder('Notes');
  await book.moveNote(id, target);
  const moved = book.data.notes.find(note => note.id === id)!;
  expect(moved.source).toBe(original.replace('](.attachments/', '](../随记/.attachments/'));
  await book.save(moved.source, id, moved.source, true);
  expect(book.data.notes.find(note => note.id === id)).toMatchObject({ title: 'A thought', source: moved.source });
  expect(memoNotes(book.data, allMemos, '')).toEqual([]);
});
