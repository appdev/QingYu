// @vitest-environment jsdom
import { afterAll, afterEach, beforeAll, expect, it, vi } from 'vitest';
import { ChangeSet, EditorSelection, EditorState, Text } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { history, undo } from '@codemirror/commands';
import { markdown, markdownLanguage } from '@codemirror/lang-markdown';
import { documentChanges, optimizeTypography } from '../src/typography';
import { formatMarkdown } from '../src/typography-format';
import { editorHistoryMenuUpdates } from '../src/editor-history-menu';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));

function apply(before: string, after: string) {
  return ChangeSet.of(documentChanges(before, after), before.length).apply(Text.of(before.split('\n'))).toString();
}

const views: EditorView[] = [];
function editor(doc: string, anchor = 0) {
  return track(new EditorView({ parent: document.body, state: EditorState.create({ doc, selection: EditorSelection.cursor(anchor),
    extensions: [markdown({ base: markdownLanguage }), history(), editorHistoryMenuUpdates] }) }));
}
function track(view: EditorView) {
  views.push(view);
  return view;
}

// jsdom 不排版，CodeMirror 量光标位置时要用到 Range.getClientRects。
const rects = Object.getOwnPropertyDescriptor(Range.prototype, 'getClientRects');
beforeAll(() => Object.defineProperty(Range.prototype, 'getClientRects', { configurable: true, value: () => [] }));
afterAll(() => {
  if (rects) Object.defineProperty(Range.prototype, 'getClientRects', rects);
  else delete (Range.prototype as Partial<Range>).getClientRects;
});

afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
  document.body.replaceChildren();
  document.head.querySelector('meta[name="qingyu-web-auth"]')?.remove();
  vi.unstubAllGlobals();
});

it('splits a rewrite into line edits that rebuild the result exactly', () => {
  const cases: [string, string][] = [
    ['用Rust写的\n第二行\n', '用 Rust 写的\n第二行\n'],
    ['a\n\n\n\nb', 'a\n\nb'],
    ['a\nb', 'a\nb\nc'],
    ['a\nb\nc', 'a\nb'],
    ['* a\n* b\n', '- a\n- b\n'],
    ['x', ''],
    ['', '# 标题\n'],
    ['等等...\n\n\n结尾', '等等……\n\n结尾\n'],
  ];
  for (const [before, after] of cases) expect(apply(before, after), JSON.stringify(before)).toBe(after);
  // 随机改动也要逐字还原。
  const words = ['中文', 'abc', '', ' ', '...', '- 项', '```'];
  for (let round = 0; round < 300; round++) {
    const pick = () => Array.from({ length: 1 + (round * 7 + 3) % 6 }, (_, i) => words[(round * 13 + i * 5) % words.length]
      + words[(round + i * 3) % words.length]).join('\n');
    const before = pick(), after = pick().split('\n').reverse().join('\n');
    expect(apply(before, after)).toBe(after);
  }
});

it('only touches the changed part of each line so the caret stays with its text', () => {
  const before = '第一行保持不变\n用Rust写的工具\n最后一行';
  const changes = documentChanges(before, '第一行保持不变\n用 Rust 写的工具\n最后一行');
  expect(changes).toEqual([{ from: 9, to: 13, insert: ' Rust ' }]);
});

it('formats Markdown like MiaoYan without breaking links, math or code', async () => {
  const source = '# 标题\n* 列表\n* 列表二\n\n链接[[车机ADB]]，公式$E=mc^2$，*强调*\n\n```js\nconst a   =  1\n```\n\n$$\na_1\n$$';
  const formatted = await formatMarkdown(source);
  expect(formatted).toBe('# 标题\n\n- 列表\n- 列表二\n\n链接[[车机ADB]]，公式$E=mc^2$，_强调_\n\n```js\nconst a   =  1\n```\n\n$$\na_1\n$$');
  expect(await formatMarkdown('# 已经整齐\n')).toBe('# 已经整齐\n');
});

it('optimizes as one undoable step and offers undo in a notice', async () => {
  const source = '# 标题\n* 一\n* 二';
  const view = editor(source, source.indexOf('二'));
  const notify = vi.fn();
  await optimizeTypography(view, notify);
  expect(view.state.doc.toString()).toBe('# 标题\n\n- 一\n- 二');
  expect(view.state.sliceDoc(view.state.selection.main.head, view.state.selection.main.head + 1)).toBe('二');
  expect(notify).not.toHaveBeenCalled();
  expect(view.dom.querySelector('.editor-history-notice')?.textContent).toContain('排版已优化');
  undo(view);
  expect(view.state.doc.toString()).toBe(source);
});

it('keeps typing that happens while formatting runs', async () => {
  const view = editor('* 一');
  const notify = vi.fn();
  const running = optimizeTypography(view, notify);
  view.dispatch({ changes: { from: view.state.doc.length, insert: '二' }, userEvent: 'input.type' });
  await running;
  expect(view.state.doc.toString()).toBe('* 一二');
  expect(notify).toHaveBeenCalledWith('正文已改动，已取消排版');
});

it('reports when nothing needs to change and leaves read-only notes alone', async () => {
  const view = editor('# 已经整齐\n');
  const notify = vi.fn();
  await optimizeTypography(view, notify);
  expect(notify).toHaveBeenCalledWith('排版无需调整');
  expect(view.dom.querySelector('.editor-history-notice')).toBeNull();

  const locked = track(new EditorView({ parent: document.body, state: EditorState.create({ doc: '* 一', extensions: EditorState.readOnly.of(true) }) }));
  await optimizeTypography(locked, notify);
  expect(locked.state.doc.toString()).toBe('* 一');
});

it('cleans spacing on the server before formatting, and reports failures without changing the note', async () => {
  const meta = document.createElement('meta');
  meta.name = 'qingyu-web-auth'; meta.content = 'enabled';
  document.head.append(meta);
  const fetch = vi.fn(async (_url: string, init: RequestInit) => {
    const { source } = JSON.parse(String(init.body)) as { source: string };
    return new Response(JSON.stringify(source.replace('用Rust', '用 Rust')));
  });
  vi.stubGlobal('fetch', fetch);
  const view = editor('* 用Rust写的');
  const notify = vi.fn();
  await optimizeTypography(view, notify);
  expect(fetch).toHaveBeenCalledWith('/api/web/typography', expect.objectContaining({ method: 'POST' }));
  // 服务器先加空格，Prettier 再把列表符号统一成 `-`。
  expect(view.state.doc.toString()).toBe('- 用 Rust写的');
  expect(view.dom.querySelector('.editor-history-notice')?.textContent).toContain('排版已优化');

  vi.stubGlobal('fetch', vi.fn(async () => new Response('', { status: 500 })));
  vi.spyOn(console, 'warn').mockImplementation(() => {});
  await optimizeTypography(view, notify);
  expect(notify).toHaveBeenCalledWith('排版失败', true);
  expect(view.state.doc.toString()).toBe('- 用 Rust写的');
});

it('still formats Markdown when the library has no server to clean spacing', async () => {
  const fetch = vi.fn();
  vi.stubGlobal('fetch', fetch);
  const view = editor('* 用Rust写的');
  await optimizeTypography(view, vi.fn());
  expect(fetch).not.toHaveBeenCalled();
  expect(view.state.doc.toString()).toBe('- 用Rust写的');
});
