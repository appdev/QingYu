// @vitest-environment jsdom
import './fixtures/chinese-ui';
import { readFileSync } from 'node:fs';
import { EditorView } from '@codemirror/view';
import { expect, it, vi } from 'vitest';
import { createMemoView } from '../src/memo-view';
import { allMemos, memoNotes } from '../src/memo';
import { Notebook, NoteSaveConflict } from '../src/notebook';
import { createSidebar } from '../src/sidebar';

vi.hoisted(() => {
  vi.stubGlobal('matchMedia', () => Object.assign(new EventTarget(), { matches: false, addListener() {}, removeListener() {} }));
});

function testBook(root?: string) {
  const storage = new Map<string, string>();
  const book = new Notebook({ getItem: key => storage.get(key) ?? null, setItem: (key, value) => { storage.set(key, value); } }, '# 普通笔记');
  if (root) vi.spyOn(book, 'root', 'get').mockReturnValue(root);
  return book;
}

function testView(getBook: () => Notebook, options: Partial<Parameters<typeof createMemoView>[1]> = {}) {
  document.body.innerHTML = '<button id="memo-search-button"></button><section id="memo-pane" hidden></section>';
  const error = vi.fn();
  const view = createMemoView(getBook, { error, changed: () => view.refresh(), filterChanged() {},
    openNote: async () => {}, openLink() {}, copyText: async () => {}, ...options });
  view.show(allMemos);
  return { view, error };
}

const inputField = (selector = '.memo-composer .cm-content') => document.querySelector<HTMLElement>(selector)!;
const editorOf = (input: HTMLElement) => EditorView.findFromDOM(input.closest<HTMLElement>('.cm-editor')!)!;
const valueOf = (input = inputField()) => editorOf(input).state.doc.toString();
function typeIn(input: HTMLElement, text: string) {
  const editor = editorOf(input);
  editor.dispatch({ changes: { from: 0, to: editor.state.doc.length, insert: text }, selection: { anchor: text.length } });
}
function editCard() {
  document.querySelector('.memo-card-body')!.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
  return inputField('.memo-edit .cm-content');
}
function dropImages(...names: string[]) {
  const files = names.map(name => Object.assign(new File(['image'], name, { type: 'image/webp' }), {
    arrayBuffer: async () => new ArrayBuffer(1),
  }));
  const event = new Event('drop', { bubbles: true, cancelable: true });
  Object.defineProperty(event, 'dataTransfer', { value: { files } });
  inputField().dispatchEvent(event);
}

it('releases the microphone if permission arrives after leaving Jots', async () => {
  const book = testBook('/recording-test');
  const { view, error } = testView(() => book);
  let grant!: (stream: MediaStream) => void;
  const stopTrack = vi.fn();
  const request = vi.fn(() => new Promise<MediaStream>(resolve => { grant = resolve; }));
  const mediaDevices = Object.getOwnPropertyDescriptor(navigator, 'mediaDevices');
  Object.defineProperty(navigator, 'mediaDevices', { configurable: true, value: { getUserMedia: request } });
  const previousRecorder = window.MediaRecorder;
  window.MediaRecorder = class { static isTypeSupported() { return true; } } as unknown as typeof MediaRecorder;
  try {
    inputField().focus();
    document.querySelector<HTMLButtonElement>('.memo-add')!.click();
    const record = await vi.waitFor(() => {
      const item = document.querySelector<HTMLButtonElement>('[role=menuitem][aria-label="录制音频"]');
      expect(item).not.toBeNull();
      return item!;
    });
    record.click();
    await vi.waitFor(() => expect(request).toHaveBeenCalledWith({ audio: true }));
    expect(document.querySelector<HTMLElement>('.memo-recording')?.dataset.state).toBe('preparing');
    expect(document.querySelector('.memo-composer')?.hasAttribute('data-recording')).toBe(true);
    view.hide();
    grant({ getTracks: () => [{ stop: stopTrack }] } as unknown as MediaStream);
    await vi.waitFor(() => expect(stopTrack).toHaveBeenCalledOnce());
    expect(document.querySelector('.memo-recording')?.hasAttribute('hidden')).toBe(true);
    expect(document.querySelector<HTMLElement>('.memo-recording')?.dataset.state).toBeUndefined();
    expect(document.querySelector('.memo-composer')?.hasAttribute('data-recording')).toBe(false);
    expect(document.querySelector('.memo-preview')).toBeNull();
    expect(error).not.toHaveBeenCalled();
  } finally {
    view.hide(); window.MediaRecorder = previousRecorder;
    if (mediaDevices) Object.defineProperty(navigator, 'mediaDevices', mediaDevices);
    else Reflect.deleteProperty(navigator, 'mediaDevices');
  }
});

it('stages a file from the Apple system menu without a WebView picker', async () => {
  const book = testBook('/native-picker-test');
  const pickAttachments = vi.fn(async () => [new File(['content'], '报告.txt', { type: 'text/plain' })]);
  const invoke = vi.fn(async () => ({ id: 1 }));
  const platform = vi.spyOn(navigator, 'platform', 'get').mockReturnValue('iPhone');
  Object.defineProperty(window, '__TAURI_INTERNALS__', { value: { invoke }, configurable: true });
  document.documentElement.setAttribute('data-mobile', '');
  const { view } = testView(() => book, { pickAttachments });
  try {
    document.querySelector<HTMLButtonElement>('.memo-add')!.click();
    await vi.waitFor(() => expect(document.querySelector('.memo-preview-file')?.textContent).toBe('报告.txt'));
    expect(invoke).toHaveBeenCalledWith('show_native_menu', expect.objectContaining({
      menu: expect.objectContaining({ sections: [[
        expect.objectContaining({ text: '添加附件' }),
        expect.objectContaining({ text: '插入图片' }),
        expect.objectContaining({ text: '录制音频' }),
        expect.objectContaining({ text: '待办' }),
      ]] }),
    }), undefined);
    expect(pickAttachments).toHaveBeenCalledWith(false, 256 * 1024 * 1024);
    expect(document.querySelector('[role=menu]')).toBeNull();
  } finally {
    view.hide(); platform.mockRestore();
    Reflect.deleteProperty(window, '__TAURI_INTERNALS__');
    document.documentElement.removeAttribute('data-mobile');
  }
});

it('loads a saved native recording on demand and revokes its playback URL on exit', async () => {
  const book = testBook('/audio-playback-test');
  const source = '.attachments/clip/voice-2026-11-27T13-44-00-000.webm';
  const id = await book.createMemo(`[Recording](${source})`);
  const readAudio = vi.fn(async () => new ArrayBuffer(4));
  const { view } = testView(() => book, { readAudio });
  const { createObjectURL, revokeObjectURL } = URL;
  URL.createObjectURL = vi.fn(() => 'blob:recording');
  URL.revokeObjectURL = vi.fn();
  const play = vi.spyOn(HTMLMediaElement.prototype, 'play').mockResolvedValue();
  try {
    expect(readAudio).not.toHaveBeenCalled();
    expect(document.querySelector('.memo-card-body a')).toBeNull();
    const menuEvent = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 20, clientY: 20 });
    document.querySelector('.memo-audio')!.dispatchEvent(menuEvent);
    expect(menuEvent.defaultPrevented).toBe(true);
    document.querySelector<HTMLButtonElement>('.memo-play-audio')!.click();
    await vi.waitFor(() => expect(play).toHaveBeenCalledOnce());
    expect(readAudio).toHaveBeenCalledWith(id, source);
    expect(document.querySelector('audio')?.getAttribute('src')).toBe('blob:recording');
    view.hide();
    expect(URL.revokeObjectURL).toHaveBeenCalledWith('blob:recording');
  } finally { view.hide(); play.mockRestore(); Object.assign(URL, { createObjectURL, revokeObjectURL }); }
});

it('renders Markdown in the composer and saves the original Markdown', async () => {
  const book = testBook('/rich-memo-test');
  const { view } = testView(() => book);
  const source = '**粗体**\n\n- [ ] 待办\n\n下一行';
  try {
    const input = inputField();
    expect(input.ariaLabel).toBe('想记点什么？');
    typeIn(input, source);
    expect(input.querySelector('.cm-markra-strong')?.textContent).toBe('粗体');
    expect(input.querySelector('.cm-markra-task-checkbox')).not.toBeNull();
    view.save();
    await vi.waitFor(() => expect(memoNotes(book.data, allMemos, '')[0]?.source).toBe(source));
    expect(valueOf()).toBe('');
  } finally { view.hide(); }
});

it('restores an inline draft after leaving or reloading, and preserves it on a typed conflict', async () => {
  const book = testBook('/edit-test');
  const id = await book.createMemo('Saved body');
  const first = testView(() => book);
  typeIn(editCard(), 'Uncommitted edit');
  first.view.hide();
  first.view.show(allMemos);
  expect(document.querySelector('.memo-edit')).toBeNull();
  expect(valueOf(editCard())).toBe('Uncommitted edit');
  window.dispatchEvent(new Event('pagehide'));
  first.view.hide();

  const second = testView(() => book);
  try {
    const input = editCard();
    expect(valueOf(input)).toBe('Uncommitted edit');
    vi.spyOn(book, 'save').mockRejectedValueOnce(new NoteSaveConflict('Different wording'));
    input.focus();
    second.view.save();
    await vi.waitFor(() => expect(second.error).toHaveBeenCalled());
    expect(document.querySelector('.memo-edit')).toBeNull();
    expect(valueOf(editCard())).toBe('Uncommitted edit');
    second.view.save();
    await vi.waitFor(() => expect(book.data.notes.find(note => note.id === id)?.source).toBe('Uncommitted edit'));
    expect(localStorage.getItem('qingyu.memo.edit:' + JSON.stringify(['qingyu.memo.draft:/edit-test', id]))).toBeNull();
  } finally { second.view.hide(); }
});

it('clears the submitted library draft even if the user switches libraries while saving', async () => {
  const original = testBook('/origin-test'), other = testBook('/other-test');
  let current = original;
  const { view } = testView(() => current);
  const create = original.createMemo.bind(original);
  let release!: () => void;
  const gate = new Promise<void>(resolve => { release = resolve; });
  vi.spyOn(original, 'createMemo').mockImplementationOnce(async source => { await gate; return create(source); });
  try {
    typeIn(inputField(), 'Already submitted');
    view.save();
    current = other; view.refresh();
    typeIn(inputField(), 'New draft in another library');
    release();
    await vi.waitFor(() => expect(memoNotes(original.data, allMemos, '')).toHaveLength(1));
    expect(valueOf()).toBe('New draft in another library');
    current = original; view.refresh();
    expect(valueOf()).toBe('');
    view.save();
    expect(memoNotes(original.data, allMemos, '')).toHaveLength(1);
  } finally { view.hide(); }
});

it('retries failed images on the original record and leaves newer text available to save separately', async () => {
  const book = testBook('/pictures-test');
  const { view, error } = testView(() => book);
  const createUrl = URL.createObjectURL, revokeUrl = URL.revokeObjectURL;
  URL.createObjectURL = vi.fn(() => 'blob:preview'); URL.revokeObjectURL = vi.fn();
  const attach = vi.spyOn(book, 'importAttachment')
    .mockResolvedValueOnce({ name: 'one.webp', path: 'one.webp', image: true })
    .mockRejectedValueOnce(new Error('upload failed'))
    .mockResolvedValueOnce({ name: 'two.webp', path: 'two.webp', image: true });
  try {
    typeIn(inputField(), 'Original record');
    dropImages('one.webp', 'two.webp'); view.save();
    const save = document.querySelector<HTMLButtonElement>('.memo-save')!;
    await vi.waitFor(() => expect(save.firstElementChild?.textContent).toBe('重试附件'));
    const original = memoNotes(book.data, allMemos, '')[0];
    typeIn(inputField(), 'Next record'); view.save();
    await vi.waitFor(() => expect(save.firstElementChild?.textContent).toBe('记下'));
    expect(error).toHaveBeenCalledTimes(1);
    expect(attach.mock.calls.map(([id]) => id)).toEqual([original.id, original.id, original.id]);
    expect(memoNotes(book.data, allMemos, '')).toHaveLength(1);
    expect(book.data.notes.find(note => note.id === original.id)?.source).toContain('![two.webp](two.webp)');
    expect(valueOf()).toBe('Next record');
    view.save();
    await vi.waitFor(() => expect(memoNotes(book.data, allMemos, '')).toHaveLength(2));
    expect(memoNotes(book.data, allMemos, '').find(note => note.id !== original.id)?.source).toBe('Next record');
  } finally { view.hide(); URL.createObjectURL = createUrl; URL.revokeObjectURL = revokeUrl; }
});

it('reports discarded images in browser-only libraries', () => {
  const book = testBook();
  const { view, error } = testView(() => book);
  try {
    dropImages('photo.webp');
    expect(error).toHaveBeenCalledWith(expect.stringContaining('当前笔记库不支持附件'));
    expect(document.querySelector('.memo-preview')).toBeNull();
  } finally { view.hide(); }
});

it('places Jots above the folder group and keeps saved jots out of the note list', async () => {
  vi.stubGlobal('CSS', { escape: (value: string) => value });
  document.body.innerHTML = readFileSync('index.html', 'utf8').match(/<body[^>]*>([\s\S]*)<\/body>/)![1];
  const storage = new Map<string, string>();
  const book = new Notebook({ getItem: key => storage.get(key) ?? null, setItem: (key, value) => { storage.set(key, value); } }, '# 普通笔记');
  const jotId = await book.createMemo('带标签的记录 #生活');
  const error = vi.fn();
  const view = createMemoView(() => book, { error, changed: () => sidebar.refresh(),
    filterChanged: filter => sidebar.memoFilterChanged(filter), openNote: async () => {}, openLink() {}, copyText: async () => {} });
  const sidebar = createSidebar(() => book, {
    memoView: view, select: async () => true, create: async () => true, save: async () => true, reload() {}, error,
  });
  try {
    const row = document.querySelector<HTMLButtonElement>('.memo-row')!;
    expect(row.closest('.memo-sidebar')?.nextElementSibling?.className).toBe('sidebar-brand');
    expect(row.textContent).toBe('随记1');
    expect(document.querySelector('.folder-row[data-folder-id=""]')?.textContent?.trim()).toBe('全部笔记1');
    expect(document.querySelector('#notes-list')?.textContent).not.toContain('带标签的记录');
    row.click();
    await vi.waitFor(() => expect(document.querySelector('#app-shell')?.getAttribute('data-view')).toBe('memos'));
    expect(document.querySelector('.memo-card')?.getAttribute('data-memo-id')).toBe(jotId);
    document.querySelector<HTMLButtonElement>('.memo-card .memo-tag')!.click();
    expect(document.querySelector('#note-title')?.textContent).toBe('随记 · #生活');
    document.querySelector<HTMLButtonElement>('.folder-row[data-folder-id=""]')!.click();
    await vi.waitFor(() => expect(document.querySelector('#app-shell')?.hasAttribute('data-view')).toBe(false));
    expect(error).not.toHaveBeenCalled();
  } finally {
    view.hide(); vi.unstubAllGlobals(); document.body.replaceChildren();
  }
});

it('supports tag autocomplete, quick filter pills, search highlights, and drag feedback', async () => {
  const book = testBook('/polish-test');
  await book.createMemo('Morning ideas #design\n\n- [ ] polish UI');
  await book.createMemo('Evening reading #books');
  const { view } = testView(() => book);
  try {
    const saveButton = document.querySelector<HTMLButtonElement>('.memo-save')!;
    const keys = [...saveButton.querySelectorAll('kbd')].map(key => key.textContent);
    const mac = /Mac|iPad|iPhone/i.test(navigator.platform);
    expect(keys).toEqual(mac ? ['⌘', '↵'] : ['Ctrl', 'Enter']);
    expect(saveButton.getAttribute('aria-keyshortcuts')).toBe(mac ? 'Meta+Enter' : 'Control+Enter');

    // Quick filter pills
    const pills = [...document.querySelectorAll<HTMLButtonElement>('.memo-quick-pill')];
    expect(pills.map(pill => pill.textContent)).toEqual(['全部', '待办 1', '#books', '#design']);
    pills[1]!.click();
    expect(document.querySelectorAll('.memo-card')).toHaveLength(1);
    pills[0]!.click();
    expect(document.querySelectorAll('.memo-card')).toHaveLength(2);

    // Search count and highlight
    view.focusSearch();
    expect(document.querySelector('#memo-pane')?.hasAttribute('data-searching')).toBe(true);
    const search = document.querySelector<HTMLInputElement>('.memo-search')!;
    search.value = 'morning';
    search.dispatchEvent(new Event('input'));
    await vi.waitFor(() => expect(document.querySelector('.memo-search-count')?.textContent).toBe('1 条结果'));
    expect(document.querySelector('mark.memo-highlight')?.textContent).toBe('Morning');
    search.value = 'design';
    search.dispatchEvent(new Event('input'));
    await vi.waitFor(() => expect(document.querySelector('.memo-tag[data-matched]')?.textContent).toBe('#design'));
    view.closeSearch();
    expect(document.querySelector('mark.memo-highlight')).toBeNull();
    expect(document.querySelector('.memo-tag[data-matched]')).toBeNull();

    // Drag feedback
    const form = document.querySelector<HTMLFormElement>('.memo-composer')!;
    const dragEnter = new Event('dragenter', { bubbles: true });
    Object.defineProperty(dragEnter, 'dataTransfer', { value: { types: ['Files'] } });
    form.dispatchEvent(dragEnter);
    expect(form.hasAttribute('data-dragging')).toBe(true);
    form.dispatchEvent(new Event('dragleave', { bubbles: true }));
    expect(form.hasAttribute('data-dragging')).toBe(false);

    // # tag autocomplete
    const field = inputField();
    field.focus();
    typeIn(field, 'Note #de');
    const options = [...document.querySelectorAll<HTMLButtonElement>('.memo-tag-option')];
    expect(options.map(option => option.textContent)).toEqual(['#design']);
    field.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    expect(valueOf()).toBe('Note #design ');
    expect(document.querySelector<HTMLElement>('.memo-tag-suggest')?.hidden).toBe(true);
  } finally { view.hide(); }
});
