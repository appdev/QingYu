// @vitest-environment jsdom
import './fixtures/chinese-ui';
import { EditorView } from '@codemirror/view';
import { afterEach, beforeEach, expect, it, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { isBlankNote, Notebook, type Note } from '../src/notebook';

vi.hoisted(() => {
  vi.stubGlobal('matchMedia', () => ({ matches: false, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }));
});
const { createSidebar } = await import('../src/sidebar');
const { createRichEditor } = await import('../src/rich-editor');

beforeEach(() => {
  document.body.innerHTML = readFileSync('index.html', 'utf8').match(/<body[^>]*>([\s\S]*)<\/body>/)![1];
  localStorage.clear(); vi.stubGlobal('CSS', { escape: (text: string) => text });
});
afterEach(() => { document.body.replaceChildren(); });

function sidebar(blank: boolean) {
  const book = new Notebook({ getItem: () => null, setItem: () => undefined }, '# 已有笔记');
  const create = vi.fn(async () => true), focusEditor = vi.fn();
  createSidebar(() => book, { async select() { return true; }, create, focusEditor, blankNote: () => blank, async save() { return true; }, reload() {}, error: vi.fn() });
  return { create, focusEditor };
}

it('opens a new note with the cursor in its text instead of renaming it in the toolbar, as Pinto does', async () => {
  const { create, focusEditor } = sidebar(false);
  document.querySelector<HTMLButtonElement>('#new-note-button')!.click();
  await vi.waitFor(() => expect(create).toHaveBeenCalledTimes(1));
  expect(focusEditor).not.toHaveBeenCalled();
  // 光标留在正文里（createNote 已聚焦编辑器），顶部标题栏不再弹出改名输入框。
  expect(document.querySelector('#rename-note')).toBeNull();
});

it('puts the cursor back in an untouched new note instead of stacking another blank one', async () => {
  const { create, focusEditor } = sidebar(true);
  document.querySelector<HTMLButtonElement>('#new-empty-note')!.click();
  await vi.waitFor(() => expect(focusEditor).toHaveBeenCalledTimes(1));
  expect(create).not.toHaveBeenCalled();
});

it('counts only an empty, editable note outside the trash as blank', () => {
  const note: Note = { id: 'n', folderId: 'default', source: '', createdAt: 0, updatedAt: 0 };
  expect(isBlankNote(note)).toBe(true);
  // 正在输入的内容还没保存时以编辑区为准。
  expect(isBlankNote(note, '我')).toBe(false);
  expect(isBlankNote({ ...note, source: ' ' })).toBe(false);
  expect(isBlankNote({ ...note, trashed: true })).toBe(false);
  expect(isBlankNote({ ...note, readOnly: true })).toBe(false);
  expect(isBlankNote(undefined)).toBe(false);
});

it('hints “标题” in an empty note and drops the hint once there is text', () => {
  const root = document.body.appendChild(document.createElement('div'));
  root.id = 'rich-editor';
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate('', 'blank');
    expect(root.querySelector('.cm-placeholder')?.textContent).toBe('标题');
    document.documentElement.lang = 'en';
    editor.refreshLanguage();
    expect(root.querySelector('.cm-placeholder')?.textContent).toBe('Title');
    view.dispatch({ changes: { from: 0, insert: '买菜' } });
    document.documentElement.lang = 'ja';
    editor.refreshLanguage();
    expect(view.state.doc.toString()).toBe('买菜');
    expect(root.querySelector('.cm-placeholder')).toBeNull();
    // 只读的笔记（如废纸篓里的）不提示。
    editor.activate('', 'read-only', undefined, undefined, true);
    expect(root.querySelector('.cm-placeholder')).toBeNull();
  } finally { document.documentElement.lang = 'zh-CN'; view.destroy(); root.remove(); }
});

it('lays out the empty line like the title so the caret lines up with the hint', () => {
  // jsdom 不排版：比对样式表，空提示所在行和标题行用同一套字号、粗细和行高。
  const css = readFileSync('src/rich-editor.css', 'utf8');
  const sharedRule = css.match(/:is\(#rich-editor, #note-history-editor\) :is\(\.cm-markra-h1, \.cm-markra-note-title\),\s*#rich-editor \.cm-line:has\(> \.cm-placeholder\)\s*\{([^}]+)\}/)?.[1];
  expect(sharedRule).toContain('font-size: 1.76em;');
  expect(sharedRule).toContain('font-size: round(nearest, 1.76em, 1px);');
  expect(sharedRule).toContain('font-weight: 700; line-height: 1.4;');
  // 提示本身不另设字号，跟着这一行走。
  expect(css).not.toMatch(/#rich-editor \.cm-placeholder \{[^}]*font-size/);
});
