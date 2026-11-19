// @vitest-environment jsdom
import { afterEach, expect, it, vi } from 'vitest';
import { createNoteMenu, noteInformation } from '../src/note-menu';
import { Notebook } from '../src/notebook';

vi.hoisted(() => { Object.assign(globalThis, { matchMedia: () => ({ matches: false, addEventListener() {}, removeEventListener() {} }) }); });

afterEach(() => { document.body.replaceChildren(); vi.unstubAllGlobals(); vi.restoreAllMocks(); });

it.each([
  ['MacIntel', '在 Finder 中显示'],
  ['Win32', '在文件资源管理器中显示'],
  ['Linux x86_64', '在文件管理器中显示'],
  ['Web', null],
])('replaces Markdown export with file location on %s', async (platform, label) => {
  vi.stubGlobal('CSS', { escape: (text: string) => text });
  vi.spyOn(navigator, 'platform', 'get').mockReturnValue(platform);
  const invoke = vi.fn(async () => undefined);
  if (label) vi.stubGlobal('__TAURI_INTERNALS__', { invoke });
  const book = new Notebook({ getItem: () => null, setItem: () => undefined }, '# Current');
  vi.spyOn(book, 'root', 'get').mockReturnValue('/test/notes');
  book.data.notes.push({ ...book.current, id: 'other', path: '子文件夹/另一篇.md' });
  const menu = createNoteMenu(() => book, { save: async () => true, change: async run => { await run(); }, error: vi.fn() });
  await menu('other', 200, 100);
  expect(document.querySelector('[aria-label="导出"]')).toBeNull();
  const item = [...document.querySelectorAll<HTMLButtonElement>('.note-context-menu [role=menuitem]')]
    .find(button => button.ariaLabel?.startsWith('在') && button.ariaLabel.endsWith('中显示'));
  expect(item?.ariaLabel ?? null).toBe(label);
  if (item) {
    expect(item.hasAttribute('aria-haspopup')).toBe(false);
    item.click();
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('library_reveal_note', { noteId: 'other' }, undefined));
  }
});

it.each([false, true])('shows the toolbar actions with icons and a divider before trash (mobile: %s)', async mobile => {
  document.body.innerHTML = '<button id="note-more-button">更多</button>';
  const book = new Notebook({ getItem: () => null, setItem: () => undefined }, '# Note');
  await book.createFolder('目标目录');
  const menu = createNoteMenu(() => book, {
    save: async () => true, change: async run => { await run(); }, error: vi.fn(), mobile,
    openNoteWindow: async () => {},
  });
  const anchor = document.querySelector<HTMLButtonElement>('#note-more-button')!;
  anchor.focus();
  vi.spyOn(anchor, 'matches').mockImplementation(selector => selector === ':focus-visible');
  await menu(book.current!.id, 200, 100, { keyboard: false });
  expect(document.activeElement).toBe(document.querySelector('.toolbar-context-menu'));
  const items = [...document.querySelectorAll<HTMLButtonElement>('.toolbar-context-menu [role=menuitem]')];
  expect(items.map(item => item.ariaLabel)).toEqual([
    ...(!mobile ? ['在新窗口打开笔记'] : []), '置顶', '制作副本', '移到废纸篓',
  ]);
  expect(items.every(item => item.firstElementChild?.tagName === 'svg')).toBe(true);
  const separators = [...document.querySelectorAll('.toolbar-menu-items > hr')];
  expect(separators.map(separator => separator.nextElementSibling?.getAttribute('aria-label'))).toEqual(['移到废纸篓']);
  document.activeElement!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  expect(document.activeElement).toBe(anchor);
  await menu(book.current!.id, 200, 100, { keyboard: true });
  expect(document.activeElement).toBe(document.querySelector('.toolbar-context-menu [role=menuitem]'));
  if (!mobile) {
    await menu(book.current!.id, 200, 100);
    expect(document.querySelector('.note-context-menu [aria-label="版本历史…"]')).toBeNull();
    vi.spyOn(book, 'hasNoteHistory', 'get').mockReturnValue(true);
    await menu(book.current!.id, 200, 100);
    expect(document.querySelector('.note-context-menu [aria-label="版本历史…"]')).not.toBeNull();
  }
});

it('moves from a desktop note row using the sidebar folder hierarchy and order', async () => {
  vi.stubGlobal('CSS', { escape: (text: string) => text });
  const book = new Notebook({ getItem: () => null, setItem: () => undefined }, '# Note');
  const parent = await book.createFolder('Alpha');
  const first = await book.createFolder('First', parent);
  const second = await book.createFolder('Second', parent);
  const sibling = await book.createFolder('Beta');
  const note = book.current!;
  await book.moveNote(note.id, parent);
  const error = vi.fn();
  const menu = createNoteMenu(() => book, {
    save: async () => true, change: async run => { await run(); }, error,
    folderOrder: () => [sibling, parent, second, first],
  });
  const submenu = (name: string) => document.querySelector<HTMLElement>(`[role=menu][aria-label="${name}"]`)!;
  const buttons = (element: HTMLElement) => [...element.querySelectorAll<HTMLButtonElement>(':scope > button')];
  await menu(note.id, 200, 100);
  buttons(submenu('笔记操作')).find(button => button.ariaLabel === '移动到…')!.click();
  const destinations = buttons(submenu('移动到…'));
  expect(destinations.map(button => button.ariaLabel)).toEqual(['备忘录', 'Beta', 'Alpha']);
  destinations[2]!.click();
  const nested = buttons(submenu('Alpha'));
  expect(nested.map(button => button.ariaLabel)).toEqual(['Alpha', 'Second', 'First']);
  expect(nested[0]!.disabled).toBe(true);
  expect(submenu('Alpha').querySelectorAll(':scope > hr')).toHaveLength(1);
  nested[1]!.click();
  await vi.waitFor(() => expect(book.current!.folderId).toBe(second));
  expect(book.current!.source).toBe(note.source);
  expect(error).not.toHaveBeenCalled();
});

it.each([false, true])('opens a mobile folder sheet with working cancel and move (trashed: %s)', async trashed => {
  vi.stubGlobal('CSS', { escape: (text: string) => text });
  const { showModal, close } = HTMLDialogElement.prototype;
  HTMLDialogElement.prototype.showModal = function () { this.open = true; };
  HTMLDialogElement.prototype.close = function () {
    this.open = false; this.dispatchEvent(new Event('close'));
  };
  try {
    const book = new Notebook({ getItem: () => null, setItem: () => undefined }, '# Note');
    const parent = await book.createFolder('父目录');
    const child = await book.createFolder('子目录', parent);
    const note = book.current!;
    await book.moveNote(note.id, parent);
    if (trashed) await book.trashNote(note.id);
    vi.stubGlobal('__TAURI_INTERNALS__', {});
    vi.spyOn(book, 'root', 'get').mockReturnValue('/test/notes');
    book.data.notes.find(item => item.id === note.id)!.path = '子文件夹/Note.md';
    const save = vi.fn(async () => true), error = vi.fn();
    const menu = createNoteMenu(() => book, { save, change: async run => { await run(); }, error, mobile: true });
    const open = async () => {
      await menu(note.id, 0, 0);
      expect(document.querySelector('.context-sheet [aria-label$="中显示"], .context-sheet [aria-label="导出"]')).toBeNull();
      document.querySelector<HTMLButtonElement>('[role=menuitem][aria-label="移动"]')!.click();
      return vi.waitFor(() => {
        const dialog = document.querySelector<HTMLDialogElement>('.note-move-sheet[open]');
        expect(dialog).not.toBeNull();
        return dialog!;
      });
    };
    let dialog = await open();
    expect(document.querySelector('.context-sheet')).toBeNull();
    const destinations = [...dialog.querySelectorAll<HTMLButtonElement>('li > button')];
    expect(destinations.map(button => button.textContent?.trim())).toEqual(['备忘录', '父目录', '子目录']);
    expect(destinations[1]!.disabled).toBe(!trashed);
    expect(destinations[2]!.style.getPropertyValue('--folder-depth')).toBe('1');
    dialog.querySelector<HTMLButtonElement>('[aria-label="取消"]')!.click();
    expect(dialog.isConnected).toBe(false);
    expect(save).not.toHaveBeenCalled();
    dialog = await open();
    dialog.querySelectorAll<HTMLButtonElement>('li > button')[2]!.click();
    await vi.waitFor(() => expect(book.data.notes.find(item => item.id === note.id)).toMatchObject({ folderId: child, source: note.source, trashed: false }));
    expect(save).toHaveBeenCalledOnce();
    expect(error).not.toHaveBeenCalled();
  } finally { Object.assign(HTMLDialogElement.prototype, { showModal, close }); }
});

it('matches MiaoYan CJK/Latin word counts, frontmatter exclusion and backlink title deduplication', () => {
  const book = new Notebook({ getItem: () => null, setItem: () => undefined }, "---\ntitle: 隐藏\n---\n# 中文 don't 2026 カナ 한글 🙂");
  const note = book.current!;
  note.title = '目标';
  book.data.notes.push({ ...note, id: 'ref-a', title: '引用者', source: '[[ 目标 ]] [[目标]]' }, { ...note, id: 'ref-b', title: '引用者', source: '[[目标]]' },
    { ...note, id: 'trash', title: '废纸篓引用', source: '[[目标]]', trashed: true }, { ...note, id: 'alias', title: '别名', source: '[[目标|别名]]' },
    { ...note, id: 'code', title: '代码示例', source: '`[[目标]]`' });
  const info = noteInformation(book.data, note.id);
  expect(info.words).toBe(8);
  expect(info.backlinks).toEqual(['别名', '引用者']);
  expect(info.modified).toMatch(/\d{4}\/\d{2}\/\d{2} \d{2}:\d{2}/);
});

it('edits the title inline, cancels with Escape, keeps source/history and dismisses the nonmodal info panel', async () => {
  vi.stubGlobal('CSS', { escape: (text: string) => text });
  const book = new Notebook({ getItem: () => null, setItem: () => undefined }, '# 正文标题\r\n\r\n正文不改\r\n');
  await book.save(book.current!.source);
  const note = book.current!, before = note.source;
  document.body.innerHTML = '<h1 id="note-title">正文标题</h1><div id="notes-list"></div>';
  const row = document.createElement('button'); row.className = 'note-row'; row.dataset.noteId = note.id; row.setAttribute('aria-selected', 'true'); document.querySelector('#notes-list')!.append(row);
  const change = vi.fn(async (mutate: () => Promise<void>) => { await mutate(); });
  const error = vi.fn(), save = vi.fn(async () => true), menu = createNoteMenu(() => book, { save, change, error });
  const open = async (name: string) => { await menu(note.id, 200, 100); [...document.querySelectorAll<HTMLButtonElement>('[role=menuitem]')].find(button => button.ariaLabel === name)!.click(); };
  await open('重命名');
  let input = document.querySelector<HTMLInputElement>('#rename-note')!;
  expect(input.parentElement!.id).toBe('note-title'); expect(document.querySelector('dialog')).toBeNull();
  input.value = '新名称'; input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
  await vi.waitFor(() => expect(book.current!.title).toBe('新名称'));
  await vi.waitFor(() => expect(document.querySelector('#rename-note')).toBeNull());
  expect(book.current!.source).toBe(before); expect(change.mock.calls[0]).toHaveLength(1);
  await open('重命名'); input = document.querySelector<HTMLInputElement>('#rename-note')!;
  input.value = '取消的名称'; input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  expect(book.current!.title).toBe('新名称'); expect(document.querySelector('#note-title')!.textContent).toBe('新名称');
  await open('显示简介');
  await vi.waitFor(() => expect(document.querySelector('.note-info')).not.toBeNull());
  expect(document.querySelector('.note-info')!.textContent).toContain('反向链接：无');
  expect(document.querySelector('.note-info')!.textContent).not.toContain('存储位置');
  document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); expect(document.querySelector('.note-info')).toBeNull();
  await open('显示简介'); await vi.waitFor(() => expect(document.querySelector('.note-info')).not.toBeNull());
  document.body.dispatchEvent(new Event('pointerdown', { bubbles: true })); expect(document.querySelector('.note-info')).toBeNull();
  expect(error).not.toHaveBeenCalled();
});
