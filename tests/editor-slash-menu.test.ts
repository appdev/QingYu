// @vitest-environment jsdom
import { EditorState } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { markdown } from '@codemirror/lang-markdown';
import { beforeEach, expect, it, vi } from 'vitest';
import { matchSlashCommands, slashCalloutType, slashCommands, slashMenu, slashQueryAt } from '../src/editor-slash-menu';
import { createRichEditor } from '../src/rich-editor';

vi.hoisted(() => {
  vi.stubGlobal('matchMedia', () => ({
    matches: false, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
  }));
});

beforeEach(() => { localStorage.clear(); });

const queryIn = (doc: string, caret = doc.length) =>
  slashQueryAt(EditorState.create({ doc, selection: { anchor: caret }, extensions: markdown() }))?.query;

it('lists the commands in Pinto order', () => {
  expect(slashCommands).toEqual([
    'title', 'heading', 'subheading', 'body', 'checklist', 'bulleted', 'dashed', 'numbered', 'quote', 'callout',
    'highlight', 'codeBlock', 'table', 'divider', 'date', 'link', 'noteLink', 'attach',
  ]);
});

it('finds the slash query only at a line start or after a space', () => {
  expect(queryIn('/')).toBe('');
  expect(queryIn('买菜 /ta')).toBe('ta');
  expect(queryIn('  /h2')).toBe('h2');
  expect(queryIn('、')).toBeUndefined();
  expect(queryIn('、清单')).toBeUndefined();
  expect(queryIn('and/or')).toBeUndefined();
  expect(queryIn('https://example.com')).toBeUndefined();
  expect(queryIn('/ ')).toBeUndefined();
  expect(queryIn('| 列 | /ta')).toBeUndefined();
  expect(queryIn('`code /ta')).toBeUndefined();
  expect(queryIn('```\n/ta')).toBeUndefined();
  expect(queryIn(`/${'a'.repeat(25)}`)).toBeUndefined();
  expect(queryIn('/table 后面', 6)).toBe('table');
});

it('ranks names before words before aliases and falls back to letters in order', () => {
  // 「heading 1」「heading 3」是标题和三级标题的别名，排在名称匹配之后。
  expect(matchSlashCommands('he')).toEqual(['heading', 'title', 'subheading']);
  expect(matchSlashCommands('list')).toEqual(['bulleted', 'dashed', 'numbered']);
  expect(matchSlashCommands('h2')).toEqual(['heading']);
  expect(matchSlashCommands('todo')).toEqual(['checklist']);
  expect(matchSlashCommands('line')).toEqual(['divider']);
  expect(matchSlashCommands('chk')).toEqual(['checklist']);
  expect(matchSlashCommands('表')).toEqual(['table', 'bulleted', 'dashed', 'numbered']);
  expect(matchSlashCommands('zzz')).toEqual([]);
});

it('leads with the recently used commands when nothing is typed', () => {
  expect(matchSlashCommands('', slashCommands, ['table', 'date']).slice(0, 3)).toEqual(['table', 'date', 'title']);
  expect(matchSlashCommands('t', slashCommands, ['table'])[0]).toBe('table');
});

it('reads a callout type from the query', () => {
  expect(slashCalloutType('warn')).toBe('warning');
  expect(slashCalloutType('caution')).toBe('warning');
  expect(slashCalloutType('call')).toBeNull();
  expect(slashCalloutType('w')).toBeNull();
});

function mount(source: string, attachFile?: () => void) {
  const root = document.body.appendChild(document.createElement('div'));
  // jsdom 算不出光标坐标，浮层会一直隐藏；菜单放进宿主容器后不依赖坐标。
  const host = document.body.appendChild(document.createElement('div'));
  const rects = Object.getOwnPropertyDescriptor(Range.prototype, 'getClientRects');
  Object.defineProperty(Range.prototype, 'getClientRects', { configurable: true, value: () => [] });
  const editor = createRichEditor(root, () => {}, { attachFile, suggestionsHost: host });
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  editor.activate(source, 'slash');
  view.focus();
  const type = (text: string) => {
    const at = view.state.selection.main.head;
    view.dispatch({ changes: { from: at, insert: text }, selection: { anchor: at + text.length }, userEvent: 'input.type' });
  };
  const press = (key: string) => view.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
  const rows = () => [...host.querySelectorAll<HTMLElement>('.editor-slash-menu:not([hidden]) [role=option]')];
  view.dispatch({ selection: { anchor: source.length } });
  return {
    view, type, press, rows,
    done() {
      view.destroy(); root.remove(); host.remove();
      if (rects) Object.defineProperty(Range.prototype, 'getClientRects', rects);
      else Reflect.deleteProperty(Range.prototype, 'getClientRects');
    },
  };
}

it('shows every command with its shortcut and runs the chosen one', () => {
  const editor = mount('# 标题\n\n', () => {});
  try {
    editor.type('/');
    expect(editor.rows().map(row => row.dataset.command)).toEqual([...slashCommands]);
    expect(editor.rows()[0].querySelector('kbd')?.textContent).toMatch(/T$/u);
    expect(editor.rows()[0].ariaSelected).toBe('true');
    editor.press('ArrowUp');
    expect(editor.rows().at(-1)?.ariaSelected).toBe('true');
    editor.press('ArrowDown');
    editor.type('dash');
    expect(editor.rows().map(row => row.dataset.command)).toEqual(['dashed']);
    editor.press('Enter');
    expect(editor.view.state.doc.toString()).toBe('# 标题\n\n- ');
    expect(editor.rows()).toEqual([]);
    // 用过的命令下次排在最前，并用一条线和其余命令隔开。
    editor.type('买菜\n');
    editor.type('/');
    expect(editor.rows()[0].dataset.command).toBe('dashed');
    expect(editor.rows()[0].nextElementSibling?.className).toBe('slash-recent-line');
  } finally { editor.done(); }
});

it('keeps a dismissed slash closed and leaves attach out when files cannot be added', () => {
  const editor = mount('');
  try {
    editor.type('/');
    expect(editor.rows().some(row => row.dataset.command === 'attach')).toBe(false);
    editor.press('Escape');
    expect(editor.rows()).toEqual([]);
    editor.type('ta');
    expect(editor.rows()).toEqual([]);
    expect(editor.view.state.doc.toString()).toBe('/ta');
  } finally { editor.done(); }
});

it('keeps the desktop popup visible through temporary focus loss and closes it when focus stays elsewhere', async () => {
  const shell = document.body.appendChild(document.createElement('main'));
  shell.id = 'app-shell';
  const outside = document.body.appendChild(document.createElement('button'));
  const view = new EditorView({ parent: shell, extensions: [slashMenu({ run() {} })] });
  const panel = shell.querySelector<HTMLElement>('.editor-slash-menu')!;
  vi.spyOn(view, 'coordsAtPos').mockReturnValue({ left: 20, right: 20, top: 0, bottom: 0 });
  try {
    view.focus();
    view.dispatch({ changes: { from: 0, insert: '/' }, selection: { anchor: 1 } });
    await vi.waitFor(() => expect(panel.hidden).toBe(false));

    shell.setAttribute('inert', '');
    view.contentDOM.blur();
    await vi.waitFor(() => expect(view.dom.classList.contains('cm-focused')).toBe(false));
    expect(panel.hidden).toBe(false);
    shell.removeAttribute('inert');
    view.focus();
    await vi.waitFor(() => expect(view.dom.classList.contains('cm-focused')).toBe(true));
    expect(panel.hidden).toBe(false);

    shell.setAttribute('inert', '');
    view.contentDOM.blur();
    await vi.waitFor(() => expect(view.dom.classList.contains('cm-focused')).toBe(false));
    expect(panel.hidden).toBe(false);
    outside.focus();
    shell.removeAttribute('inert');
    await vi.waitFor(() => expect(panel.hidden).toBe(true));
  } finally { view.destroy(); shell.remove(); outside.remove(); }
});

it('inserts dividers, typed callouts and note links from the menu', () => {
  const editor = mount('');
  try {
    editor.type('/warn');
    expect(editor.rows()[0].textContent).toContain('Warning 标注');
    editor.press('Tab');
    expect(editor.view.state.doc.toString()).toBe('> [!WARNING]\n> ');
    editor.view.dispatch({ changes: { from: 0, to: editor.view.state.doc.length }, selection: { anchor: 0 } });
    editor.type('/wiki');
    editor.press('Enter');
    expect(editor.view.state.doc.toString()).toBe('[[');
  } finally { editor.done(); }
});
