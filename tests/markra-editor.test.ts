// @vitest-environment jsdom
import { EditorState, Text } from '@codemirror/state';
import { EditorView } from '@codemirror/view';
import { foldedRanges, forceParsing } from '@codemirror/language';
import { deleteCharBackward, redo, undo, undoDepth } from '@codemirror/commands';
import { Menu } from '@tauri-apps/api/menu';
import { expect, it, vi } from 'vitest';
import { webcrypto } from 'node:crypto';
import { createRichEditor } from '../src/rich-editor';
import { readWebLinkPreview } from '../src/web-link-preview';
import { openEditorFormatPanel } from '../src/editor-format-panel';
import { connectMobileEditorHistory, runEditorHistoryMenu } from '../src/editor-history-menu';
import { restoreCopiedTableHeader } from '../src/vendor/markra/editor/codemirror/table';
import { createCellTableControls } from '../src/vendor/markra/editor/codemirror/table-controls';
import { tableColumnWidths } from '../src/vendor/markra/editor/codemirror/table-layout';
import { placeTableCellCaret } from '../src/vendor/markra/editor/codemirror/table-selection';
import { runMarkraCommand } from '../src/vendor/markra/editor/codemirror/plugin';

vi.hoisted(() => {
  vi.stubGlobal('matchMedia', () => ({
    matches: false, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {},
  }));
});

it('mounts editor suggestions in the mobile keyboard bar and closes link suggestions during composition', async () => {
  const root = document.body.appendChild(document.createElement('div'));
  const host = document.body.appendChild(document.createElement('div'));
  const rects = Object.getOwnPropertyDescriptor(Range.prototype, 'getClientRects');
  Object.defineProperty(Range.prototype, 'getClientRects', { configurable: true, value: () => [] });
  const editor = createRichEditor(root, () => {}, {
    suggestionsHost: host, suggestNoteLinks: async () => [{ title: '项目计划', target: '项目计划' }],
  });
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate('# 测试\n\n', 'keyboard-suggestions');
    view.focus();
    const end = view.state.doc.length;
    view.dispatch({ changes: { from: end, insert: '/' }, selection: { anchor: end + 1 }, userEvent: 'input.type' });
    expect(host.querySelector('.editor-slash-menu')?.hasAttribute('hidden')).toBe(false);
    expect(root.querySelector('.editor-slash-menu')).toBeNull();
    view.dispatch({ changes: { from: end, to: end + 1, insert: '[[' }, selection: { anchor: end + 2 }, userEvent: 'input.type' });
    await vi.waitFor(() => expect(host.querySelector('.note-link-completion [role=option]')?.textContent).toContain('项目计划'));
    expect(host.querySelector('.editor-slash-menu')?.hasAttribute('hidden')).toBe(true);
    view.contentDOM.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }));
    expect(host.hasAttribute('data-composing')).toBe(true);
    expect(host.querySelector('.note-link-completion')).toBeNull();
    view.contentDOM.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }));
    expect(host.hasAttribute('data-composing')).toBe(false);
    expect(view.state.doc.toString()).toBe('# 测试\n\n[[');
  } finally {
    view.destroy(); root.remove(); host.remove();
    if (rects) Object.defineProperty(Range.prototype, 'getClientRects', rects);
    else Reflect.deleteProperty(Range.prototype, 'getClientRects');
  }
});

it('keeps the selected text while applying multiple mobile formats and closing the panel', () => {
  const source = '# 标题\n\n中文正文';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const trigger = root.appendChild(document.createElement('button'));
  try {
    editor.activate(source, 'mobile-format-selection');
    view.dispatch({ selection: { anchor: source.indexOf('中文正文'), head: source.length } });
    const restore = vi.fn();
    openEditorFormatPanel(trigger, editor.formatPanelState, editor.applyFormatPanel, restore);
    const panel = document.querySelector<HTMLDialogElement>('.format-panel')!;
    const button = (action: string) => panel.querySelector<HTMLButtonElement>(`[data-format="${action}"]`)!;
    for (const action of ['bold', 'italic', 'underline', 'strike', 'highlight']) {
      expect(button(action).querySelector('svg')?.getAttribute('aria-hidden')).toBe('true');
    }
    expect(button('body').ariaPressed).toBe('true');
    expect(button('indent').disabled).toBe(true);
    button('bold').click();
    expect(button('bold').ariaPressed).toBe('true');
    button('italic').click();
    expect(button('italic').ariaPressed).toBe('true');
    expect(panel.open).toBe(true);
    expect(view.state.doc.toString()).toBe(source.replace('中文正文', '***中文正文***'));
    panel.querySelector<HTMLButtonElement>('header button')!.click();
    expect(restore).toHaveBeenCalledWith(true);
    expect(trigger.ariaExpanded).toBe('false');
    expect(view.state.sliceDoc(view.state.selection.main.from, view.state.selection.main.to)).toBe('中文正文');
  } finally {
    document.querySelector('.format-panel')?.remove();
    view.destroy(); root.remove();
  }
});

it('keeps a table selection through successive formats and isolates the source changes', () => {
  const source = '# 笔记\n\n| A  | B |\n| :---- | ---: |\n|甲\\|乙| 右侧  |\n\n下文';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(source, 'mobile-format-cell');
    const cell = root.querySelector('td')!;
    const range = document.createRange();
    range.selectNodeContents(cell);
    cell.focus();
    document.getSelection()!.removeAllRanges();
    document.getSelection()!.addRange(range);
    const format = editor.captureTableFormat()!;
    const next = (markup: string) => source.replace('甲\\|乙', markup);
    expect(format.canFormat()).toBe(true);
    const other = root.appendChild(document.createElement('button'));
    other.focus();
    format.apply('bold');
    expect(view.state.doc.toString()).toBe(next('**甲\\|乙**'));
    expect(document.activeElement).toBe(other);
    format.apply('italic');
    expect(view.state.doc.toString()).toBe(next('***甲\\|乙***'));
    expect(format.active()).toEqual(['bold', 'italic']);
    format.apply('bold');
    expect(view.state.doc.toString()).toBe(next('*甲\\|乙*'));
    expect(format.active()).toEqual(['italic']);
    expect(format.restore()).toBe(true);
    expect(document.getSelection()?.toString()).toBe('甲|乙');
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(next('***甲\\|乙***'));
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(next('**甲\\|乙**'));
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(source);
    editor.activate('另外一篇笔记', 'second-note');
    format.apply('bold');
    expect(format.restore()).toBe(false);
    expect(view.state.doc.toString()).toBe('另外一篇笔记');
  } finally { view.destroy(); root.remove(); }
});

it('applies a uniform cell format across already formatted and plain text', () => {
  const source = '| A |\n| --- |\n| **甲**乙 |';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(source, 'mobile-format-mixed-cell');
    const cell = root.querySelector('td')!;
    cell.focus();
    const range = document.createRange();
    range.selectNodeContents(cell);
    document.getSelection()!.removeAllRanges();
    document.getSelection()!.addRange(range);
    const format = editor.captureTableFormat()!;
    expect(format.active()).toEqual([]);
    format.apply('bold');
    expect(view.state.doc.toString()).toBe('| A |\n| --- |\n| **甲乙** |');
  } finally { view.destroy(); root.remove(); }
});

it.each([
  ['# 标题\n\n|文本', '# 标题\n\n```\n|文本\n```'],
  ['# 标题\n\n文本|', '# 标题\n\n```\n文本|\n```'],
  ['# 标题\n\n|', '# 标题\n\n```\n|\n```'],
  ['# 标题\n\n```|\n```', '# 标题\n\n|'],
] as const)('keeps the mobile cursor on the code content when toggling %s', (marked, expected) => {
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(marked.replace('|', ''), 'mobile-format-code');
    view.dispatch({ selection: { anchor: marked.indexOf('|') } });
    editor.applyFormatPanel('codeblock');
    expect(view.state.doc.toString()).toBe(expected.replace('|', ''));
    expect(view.state.selection.main.head).toBe(expected.indexOf('|'));
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(marked.replace('|', ''));
  } finally { view.destroy(); root.remove(); }
});

it.each([
  ['正文 li|ght', 'bold', '正文 **li|ght**'],
  ['正文 温|柔', 'italic', '正文 *温|柔*'],
  ['正文 <u>li|ght</u>', 'underline', '正文 li|ght'],
  ['正文 **rough sea|**', 'bold', '正文 **rough sea**|'],
  ['正文 ~~rough sea|~~', 'strike', '正文 ~~rough sea~~|'],
  ['正文 **|**', 'bold', '正文 |'],
  ['正文 |', 'highlight', '正文 ==|=='],
] as const)('matches Pinto inline formatting at the caret / %s / %s', (marked, action, expected) => {
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const source = marked.replace('|', '');
  try {
    editor.activate(source, 'format-at-caret');
    view.dispatch({ selection: { anchor: marked.indexOf('|') } });
    editor.format(action);
    expect(view.state.doc.toString()).toBe(expected.replace('|', ''));
    expect(view.state.selection.main.head).toBe(expected.indexOf('|'));
    expect(undoDepth(view.state)).toBe(expected.replace('|', '') === source ? 0 : 1);
  } finally { view.destroy(); root.remove(); }
});

it.each([
  ['\n\n无标记的|标题', 'title'],
  ['# 标题\n\n> [!TIP]+ 提醒\n> 带格式的|正文', 'callout'],
  ['# 标题\n\n> [!TIP] 提醒\n\n> 普通|引用', 'quote'],
  ['# 标题\n\n```md\n> [!TIP] |示例\n```', 'body'],
] as const)('reports the paragraph style at the format menu caret / %s', (marked, expected) => {
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(marked.replace('|', ''), 'format-menu-state');
    view.dispatch({ selection: { anchor: marked.indexOf('|') } });
    expect(editor.paragraphFormat()).toBe(expected);
    if (expected === 'title') expect(root.querySelector('.cm-markra-note-title')?.textContent).toBe('无标记的标题');
  } finally { view.destroy(); root.remove(); }
});

it.each([
  ['正文含 **粗体|**', '正文含 **粗体**\n|'],
  ['正文含 *斜体|*', '正文含 *斜体*\n|'],
  ['正文含 ~~删除线|~~', '正文含 ~~删除线~~\n|'],
  ['正文含 ==高亮|==', '正文含 ==高亮==\n|'],
  ['正文含 <u>下划线|</u>', '正文含 <u>下划线</u>\n|'],
  ['正文含 `代码|`', '正文含 `代码`\n|'],
  ['正文含 ***粗斜体|***', '正文含 ***粗斜体***\n|'],
  ['- [x] 已经 **完成|**', '- [x] 已经 **完成**\n- [ ] |'],
  ['> 引用 **正文|**', '> 引用 **正文**\n> |'],
  ['正文末尾 **|**', '正文末尾 \n|'],
  ['正文 **强调|** 后面', '正文 **强调\n|** 后面'],
  ['算式 2 * 3 |*', '算式 2 * 3 \n|*'],
  ['转义 \\**|**', '转义 \\**\n|**'],
  ['```md\n原样 **保留|**\n```', '```md\n原样 **保留\n|**\n```'],
])('preserves line-end inline formatting on Return in %s', (marked, expected) => {
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const source = marked.replace('|', '');
  try {
    editor.activate(source, 'return-after-format');
    view.focus();
    view.dispatch({ selection: { anchor: marked.indexOf('|') } });
    view.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    expect(view.state.doc.toString()).toBe(expected.replace('|', ''));
    expect(view.state.selection.main.head).toBe(expected.indexOf('|'));
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(source);
  } finally { view.destroy(); root.remove(); }
});

it('fits natural table columns without stretching short grids or clipping columns that can fit', () => {
  const sum = (columns: number[]) => columns.reduce((total, width) => total + width, 0);
  expect(tableColumnWidths([10, 30], 600, 13)).toEqual([64, 64]);
  expect(sum(tableColumnWidths([60, 90, 76], 360, 13))).toBeCloseTo(360);
  const tight = tableColumnWidths([65, 85, 58, 32], 320, 13);
  expect(sum(tight)).toBeCloseTo(320);
  expect(tight.every((width, index) => width >= [81, 101, 74, 48][index]!)).toBe(true);
  expect(tableColumnWidths([500, 500, 500], 340, 17)).toEqual([280, 280, 280]);
  expect(tableColumnWidths([500], 600, 24)[0]).toBeCloseTo(280 * 24 / 17);
});

it('reveals the cell entered by the caret inside a horizontally scrolled grid', () => {
  const scroller = document.body.appendChild(document.createElement('div'));
  scroller.className = 'markra-table-scroll';
  scroller.innerHTML = '<table><tbody><tr><td tabindex="0">末列</td></tr></tbody></table>';
  const cell = scroller.querySelector('td')!;
  try {
    Object.defineProperties(scroller, { scrollWidth: { value: 185 }, clientWidth: { value: 100 } });
    scroller.getBoundingClientRect = () => new DOMRect(10, 0, 100, 20);
    cell.getBoundingClientRect = () => new DOMRect(142, 0, 52, 20);
    placeTableCellCaret(cell, 1);
    expect(scroller.scrollLeft).toBe(84);
    expect(document.getSelection()?.anchorOffset).toBe(1);
  } finally { scroller.remove(); }
});

it('collapses a heading, its nested text and table without changing the source', () => {
  const source = '## 收起此节\n\n正文\n\n### 子节\n\n| 表头 |\n| --- |\n| 不应可见 |\n\n```md\n## 代码\n```\n\n## 下一节\n\n可见的正文';
  const root = document.body.appendChild(document.createElement('div'));
  const change = vi.fn();
  const editor = createRichEditor(root, change);
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(source, 'heading-fold');
    expect(forceParsing(view, view.state.doc.length)).toBe(true);
    const toggle = () => root.querySelector<HTMLButtonElement>('.qingyu-heading-fold')!;
    expect(root.querySelectorAll('.qingyu-heading-fold')).toHaveLength(3);
    view.dispatch({ selection: { anchor: source.indexOf('正文') } });
    toggle().click();
    expect(view.state.selection.main.head).toBe(source.indexOf('\n'));
    expect(root.querySelector('.cm-markra-table')).toBeNull();
    expect(root.textContent).not.toContain('子节');
    expect(root.textContent).toContain('下一节');
    expect(toggle().parentElement?.querySelector('.qingyu-heading-more')).not.toBeNull();
    expect(view.state.doc.toString()).toBe(source);
    expect(change).not.toHaveBeenCalled();

    view.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    const after = source.indexOf('\n\n## 下一节');
    expect(view.state.doc.toString()).toBe(`${source.slice(0, after)}\n${source.slice(after)}`);
    expect(view.state.selection.main.head).toBe(after + 1);
    expect(foldedRanges(view.state).size).toBe(1);
    toggle().click();
    expect(foldedRanges(view.state).size).toBe(0);
    expect(root.querySelector('.cm-markra-table')).not.toBeNull();
  } finally { view.destroy(); root.remove(); }
});

it('keeps heading controls out of the text while preserving heading formatting commands', () => {
  const source = '# 爱上大叔大叔\n\n萨达萨达撒';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(source, 'heading-control');
    view.focus();
    view.dispatch({ selection: { anchor: 4 } });
    expect(view.hasFocus).toBe(true);
    expect(root.querySelectorAll('.qingyu-heading-fold')).toHaveLength(1);
    expect(root.querySelector('.markra-heading-level-control')).toBeNull();
    expect(runMarkraCommand(view, 'block.heading.6')).toBe(true);
    expect(view.state.doc.toString()).toBe('#####' + source);
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(source);
  } finally { view.destroy(); root.remove(); }
});

it('restores the same named section after a note changes and reveals it when navigating inside', async () => {
  vi.stubGlobal('crypto', webcrypto);
  const source = '## 同名\n\n第一段\n\n## 同名\n\n第二段\n\n#### Markdown 四级\n\n内容';
  const note = 'heading-fold-restore', storageKey = `qingyu.heading-folds.${note}`;
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(source, note);
    expect(forceParsing(view, view.state.doc.length)).toBe(true);
    const toggles = root.querySelectorAll<HTMLButtonElement>('.qingyu-heading-fold');
    expect(toggles).toHaveLength(2);
    toggles[1].click();
    await vi.waitFor(() => expect(localStorage.getItem(storageKey)).toMatch(/^\["[a-f0-9]{20}"\]$/));
    editor.activate('另一篇', 'another-note');
    expect(foldedRanges(view.state).size).toBe(0);

    editor.activate(`前言\n\n${source}`, note);
    await vi.waitFor(() => expect(foldedRanges(view.state).size).toBe(1));
    expect(root.textContent).toContain('第一段');
    expect(root.textContent).not.toContain('第二段');
    editor.scrollToLine(9);
    expect(foldedRanges(view.state).size).toBe(0);
    expect(root.textContent).toContain('第二段');
  } finally {
    view.destroy(); root.remove();
    localStorage.removeItem(storageKey);
    vi.unstubAllGlobals();
  }
});

it('finds committed Chinese in a folded table, counts matches and replaces them through the search panel', () => {
  const source = '## 表格\n\n| 表头 |\n| --- |\n| 中文 |\n\n## 后文\n\n正文中文';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(source, 'find-in-folded-table');
    expect(forceParsing(view, view.state.doc.length)).toBe(true);
    root.querySelector<HTMLButtonElement>('.qingyu-heading-fold')!.click();
    expect(foldedRanges(view.state).size).toBe(1);
    editor.find();
    const panel = root.querySelector('.cm-panels-top .qingyu-find')!;
    const query = panel.querySelector<HTMLInputElement>('[main-field]')!;
    const count = panel.querySelector('output')!;
    const replace = panel.querySelector<HTMLElement>('.qingyu-find-replace')!;
    expect(document.activeElement).toBe(query);
    expect(replace.hidden).toBe(true);
    query.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }));
    query.value = 'zhong';
    query.dispatchEvent(new InputEvent('input', { bubbles: true, isComposing: true }));
    expect(count.hidden).toBe(true);
    query.value = '中文';
    query.dispatchEvent(new CompositionEvent('compositionend', { bubbles: true }));
    expect(count.value).toBe('2');
    expect(foldedRanges(view.state).size).toBe(0);
    expect(root.querySelector('td .cm-markra-search-match-current')?.textContent).toBe('中文');
    query.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    expect(view.state.selection.main.from).toBe(source.lastIndexOf('中文'));

    panel.querySelector<HTMLInputElement>('[type=checkbox]')!.click();
    expect(replace.hidden).toBe(false);
    const value = replace.querySelector('input')!;
    value.value = '已替换';
    value.dispatchEvent(new InputEvent('input', { bubbles: true }));
    replace.querySelector<HTMLButtonElement>('[data-replace=all]')!.click();
    expect(view.state.doc.toString()).toBe(source.replaceAll('中文', '已替换'));
    expect(count.value).toBe('0');
    query.blur();
    replace.querySelector<HTMLButtonElement>('.qingyu-find-done')!.click();
    expect(document.activeElement).toBe(view.contentDOM);
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(source);
  } finally { view.destroy(); root.remove(); }
});

it('forwards only the edited range and resets undo when a different note opens', () => {
  const first = '# 笔记\n\n正文\n\n| A | B |\n| --- | --- |\n| 原样 | 保留 |\n';
  let source = Text.of(first.split('\n'));
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, changes => { source = changes.apply(source); });
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(first, 'a');
    view.dispatch({ selection: { anchor: first.indexOf('正文') + 2 } });
    editor.captureSelection();
    editor.insert('中文');
    expect(source.toString()).toBe(first.replace('正文', '正文中文'));
    expect(undo(view)).toBe(true);
    expect(source.toString()).toBe(first);

    const second = '# 另一篇\n\n草稿\n';
    source = EditorState.create({ doc: second }).doc;
    editor.activate(second, 'b');
    expect(undo(view)).toBe(false);
    expect(source.toString()).toBe(second);
  } finally {
    view.destroy();
    root.remove();
  }
});

it('routes native history commands to the focused table and leaves other fields alone', () => {
  const source = '| Name |\n| --- |\n| 原文 |';
  const root = document.body.appendChild(document.createElement('div'));
  const search = document.body.appendChild(document.createElement('input'));
  search.type = 'search';
  const exec = vi.fn(() => true);
  const nativeExec = Object.getOwnPropertyDescriptor(document, 'execCommand');
  Object.defineProperty(document, 'execCommand', { configurable: true, value: exec });
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(source, 'menu-history');
    view.dispatch({ changes: { from: source.indexOf('原文'), to: source.indexOf('原文') + 2, insert: '编辑' } });
    root.querySelector('td')!.focus();
    runEditorHistoryMenu('undo');
    expect(view.state.doc.toString()).toBe(source);
    expect(root.querySelector('td')!.contains(document.activeElement) || document.activeElement === root.querySelector('table')).toBe(true);
    runEditorHistoryMenu('redo');
    expect(view.state.doc.toString()).toBe(source.replace('原文', '编辑'));
    search.focus();
    runEditorHistoryMenu('undo');
    expect(exec).toHaveBeenCalledExactlyOnceWith('undo');
    expect(view.state.doc.toString()).toBe(source.replace('原文', '编辑'));
  } finally {
    if (nativeExec) Object.defineProperty(document, 'execCommand', nativeExec);
    else Reflect.deleteProperty(document, 'execCommand');
    view.destroy();
    root.remove();
    search.remove();
  }
});

it('preserves the metadata of an old template app when editing it as ordinary Markdown', () => {
  const root = document.body.appendChild(document.createElement('div'));
  const metadata = '---\nqingyu-template-app: budget-log\nqingyu-template-data: {"expenses":[{"amount":12}]}\n---\n';
  const source = metadata + '# 收支\n\n正文';
  let markdown = Text.of(source.split('\n'));
  const editor = createRichEditor(root, changes => { markdown = changes.apply(markdown); });
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(source, 'legacy-note');
    view.dispatch({ changes: { from: view.state.doc.length, insert: '已修改' } });
    expect(markdown.toString()).toBe(source + '已修改');
  } finally { view.destroy(); root.remove(); }
});

it('shows mobile undo only for the active note and offers redo while undo is available', async () => {
  Object.defineProperty(Range.prototype, 'getClientRects', { configurable: true, value: () => [] });
  const root = document.body.appendChild(document.createElement('div'));
  const button = document.body.appendChild(document.createElement('button'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root)!;
  try {
    editor.activate('原文', 'mobile-undo-first');
    connectMobileEditorHistory(button, () => view);
    await vi.waitFor(() => expect(button.hidden).toBe(true));
    view.dispatch({ changes: { from: 0, insert: '第一段\n' } });
    view.dispatch({ changes: { from: view.state.doc.length, insert: '第二段' } });
    await vi.waitFor(() => expect(button.hidden).toBe(false));
    button.click();
    expect(view.state.doc.toString()).toBe('第一段\n原文');
    await vi.waitFor(() => expect(button.getAttribute('aria-haspopup')).toBe('menu'));
    button.dispatchEvent(new MouseEvent('contextmenu', { cancelable: true }));
    document.querySelector<HTMLButtonElement>('[role="menuitem"][aria-label="重做"]')!.click();
    await vi.waitFor(() => expect(view.state.doc.toString()).toBe('第一段\n原文第二段'));
    button.click();
    button.click();
    await vi.waitFor(() => expect(button.hidden).toBe(true));
    expect(view.state.doc.toString()).toBe('原文');
    editor.activate('另一篇', 'mobile-undo-second');
    button.click();
    expect(view.state.doc.toString()).toBe('另一篇');
  } finally {
    view.destroy(); root.remove(); button.remove();
    Reflect.deleteProperty(Range.prototype, 'getClientRects');
  }
});

it('inserts a blank grid as its own Markdown block without joining the surrounding text', () => {
  const original = '前一段\n下一段';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(original, 'table-test');
    view.dispatch({ selection: { anchor: '前一段'.length } });
    editor.format('table');
    expect(view.state.doc.toString()).toBe('前一段\n\n|  |  |\n| --- | --- |\n|  |  |\n\n下一段');
  } finally {
    view.destroy();
    root.remove();
  }
});

it('restores a missing header on a pasted table fragment without rewriting other Markdown', () => {
  const fragment = '| :--- | ---: |\n| ⌘B | Bold |\n| ⌘⇧L | Checklist |';
  expect(restoreCopiedTableHeader(fragment)).toBe(`|  |  |\n${fragment}`);
  expect(restoreCopiedTableHeader(`| Shortcut | Does |\n${fragment}`)).toBe(`| Shortcut | Does |\n${fragment}`);
  const prose = '| --- | --- |\n| unmatched |\n普通正文';
  expect(restoreCopiedTableHeader(prose)).toBe(prose);
  const mixed = `/\r\n\r\n  ${fragment.replaceAll('\n', '\r\n  ')}\r\n\r\n尾段`;
  expect(restoreCopiedTableHeader(mixed)).toBe(mixed.replace('  | :---', '  |  |  |\r\n  | :---'));
  const code = `\`\`\`md\n\n${fragment}\n\`\`\`\n\n${fragment.replace(/^/gm, '    ')}`;
  expect(restoreCopiedTableHeader(code)).toBe(code);
});

it.each([
  ['', '', ''],
  ['/\n\n', '', ''],
  ['正文\n', '', ''],
  ['', '/\n\n', '\n\n尾段'],
  ['正文', '/\n\n', '\n\n尾段'],
])('renders a pasted shortcut table (%j, %j, %j)', (before, prefix, suffix) => {
  const fragment = `${prefix}| --- | --- |\n| ⌘B | Bold |\n| ⌘⇧L | Checklist |${suffix}`;
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(before, 'paste-shortcuts');
    view.dispatch({ selection: { anchor: before.length } });
    const event = new Event('paste', { bubbles: true, cancelable: true });
    Object.defineProperty(event, 'clipboardData', { value: { getData: (type: string) => type === 'text/plain' ? fragment : '' } });
    view.contentDOM.dispatchEvent(event);
    expect(root.querySelector('.cm-markra-table tbody')?.textContent).toBe('⌘BBold⌘⇧LChecklist');
    if (prefix) expect(view.state.doc.line(1).text).toBe(before + '/');
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(before);
  } finally {
    view.destroy();
    root.remove();
  }
});

it('routes overlapping touch targets to the nearer grip and anchors its menu there', () => {
  const wrapper = document.createElement('div');
  wrapper.innerHTML = '<div><table><tr><td></td></tr></table></div>';
  const table = wrapper.querySelector('table')!;
  const cell = table.rows[0].cells[0];
  const openMenu = vi.fn();
  const controls = createCellTableControls(wrapper, table, () => cell, openMenu, () => {}, () => {});
  const row = wrapper.querySelector<HTMLButtonElement>('.qingyu-table-handle-row')!;
  const column = wrapper.querySelector<HTMLButtonElement>('.qingyu-table-handle-column')!;
  const mobile = document.documentElement.hasAttribute('data-mobile');
  document.documentElement.setAttribute('data-mobile', '');
  try {
    row.getBoundingClientRect = () => new DOMRect(0, 30, 44, 44);
    row.firstElementChild!.getBoundingClientRect = () => new DOMRect(0, 37, 16, 30);
    column.getBoundingClientRect = () => new DOMRect(22, 0, 44, 44);
    column.firstElementChild!.getBoundingClientRect = () => new DOMRect(29, 14, 30, 16);
    column.dispatchEvent(new MouseEvent('click', { clientX: 23, clientY: 43, detail: 1 }));
    expect(openMenu).toHaveBeenLastCalledWith(cell, 0, 78, 'row', row.getBoundingClientRect());
    row.dispatchEvent(new MouseEvent('click', { clientX: 42, clientY: 31, detail: 1 }));
    expect(openMenu).toHaveBeenLastCalledWith(cell, 22, 48, 'column', column.getBoundingClientRect());
    column.click();
    expect(openMenu).toHaveBeenLastCalledWith(cell, 22, 48, 'column', column.getBoundingClientRect());
  } finally {
    controls.destroy();
    document.documentElement.toggleAttribute('data-mobile', mobile);
  }
});

it.each([null, 1, 'unavailable'] as const)('uses the native iOS grip menu and handles %j', async result => {
  const source = '| 功能 |\n| --- |\n| 清单 |';
  const root = document.body.appendChild(document.createElement('div'));
  const mobile = document.documentElement.hasAttribute('data-mobile');
  document.documentElement.setAttribute('data-mobile', '');
  const native = vi.fn(async () => {
    if (result === 'unavailable') throw new Error('Unsupported iOS');
    return result;
  });
  vi.stubGlobal('__TAURI_INTERNALS__', { invoke: native });
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(source, 'native-grip-menu');
    const cell = root.querySelector('td')!;
    cell.focus();
    document.getSelection()!.collapse(cell, 0);
    const grip = root.querySelector<HTMLButtonElement>('.qingyu-table-handle-column')!;
    grip.getBoundingClientRect = () => new DOMRect(40, 60, 44, 44);
    grip.click();
    expect(native).toHaveBeenCalledWith('show_table_handle_menu', { menu: {
      frame: [40, 60, 44, 44], sections: [[
        { text: '在左侧插入列', symbol: 'arrow.left', destructive: false }, { text: '在右侧插入列', symbol: 'arrow.right', destructive: false },
      ], [], [{ text: '删除表格', symbol: 'trash', destructive: true }]],
    } }, undefined);
    if (result === 'unavailable') {
      const menu = await vi.waitFor(() => {
        const fallback = document.querySelector('[role="menu"][aria-label="列选项"]');
        expect(fallback).not.toBeNull();
        return fallback!;
      });
      menu.querySelector('button')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    } else if (result === 1) await vi.waitFor(() => expect(document.activeElement).toBe(root.querySelector('th[data-table-column="1"]')));
    else await Promise.resolve();
    expect(view.state.doc.toString()).toBe(result === 1 ? '| 功能 |  |\n| --- | --- |\n| 清单 |  |' : source);
    if (result !== 1) expect(document.activeElement).toBe(cell);
  } finally {
    view.destroy(); root.remove(); vi.unstubAllGlobals(); document.documentElement.toggleAttribute('data-mobile', mobile);
  }
});

it.each(['关闭表格面板', '在下方插入行', '更换笔记'] as const)('keeps the mobile table panel scoped to its active cell on %s', async action => {
  const source = '| 功能 |\n| --- |\n| 清单 |\n\n后文';
  const root = document.body.appendChild(document.createElement('div'));
  const mobile = document.documentElement.hasAttribute('data-mobile');
  document.documentElement.setAttribute('data-mobile', '');
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(source, 'table-panel');
    const cell = root.querySelector('td')!;
    cell.focus();
    document.getSelection()!.collapse(cell.firstChild, 1);
    const trigger = root.appendChild(document.createElement('button'));
    const open = editor.captureTableMenu(trigger);
    expect(open).toBeTypeOf('function');
    trigger.focus();
    document.getSelection()!.collapse(view.contentDOM, 0);
    open!();
    const panel = document.querySelector<HTMLDialogElement>('dialog.table-panel')!;
    expect(panel.open).toBe(true);
    expect(document.activeElement).toBe(cell);
    expect(document.getSelection()!.anchorOffset).toBe(1);
    expect(root.querySelectorAll('[data-table-panel-tint]')).toHaveLength(2);
    expect(cell.dataset.tablePanelTint).toBe('cross');
    expect(panel.querySelector<HTMLButtonElement>('[aria-label="删除列"]')?.disabled).toBe(true);
    if (action === '更换笔记') editor.activate('另一篇', 'other');
    else panel.querySelector<HTMLButtonElement>(`[aria-label="${action}"]`)!.click();
    await Promise.resolve();
    expect(panel.isConnected).toBe(false);
    expect(root.querySelector('[data-table-panel-tint]')).toBeNull();
    if (action === '更换笔记') expect(view.state.doc.toString()).toBe('另一篇');
    else if (action === '关闭表格面板') {
      expect(document.activeElement).toBe(cell);
      expect(document.getSelection()!.anchorOffset).toBe(1);
      expect(view.state.doc.toString()).toBe(source);
    } else {
      const added = root.querySelector('td[data-table-row="1"]');
      expect(document.activeElement).toBe(added);
      expect(view.state.doc.toString()).toBe(source.replace('| 清单 |', '| 清单 |\n|  |'));
    }
  } finally {
    editor.deactivate(); view.destroy(); root.remove(); document.documentElement.toggleAttribute('data-mobile', mobile);
  }
});

it('keeps the grid visible and focuses the tapped cell when WebKit places the mobile caret in source', () => {
  const source = '正文\n\n| 功能 | 快捷键 |\n| --- | --- |\n| 粗体 | ⌘B |';
  const root = document.body.appendChild(document.createElement('div'));
  const mobile = document.documentElement.hasAttribute('data-mobile');
  document.documentElement.setAttribute('data-mobile', '');
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(source, 'touch-cell-focus');
    view.focus();
    view.dispatch({ selection: { anchor: source.indexOf('功能') } });
    const cell = root.querySelectorAll<HTMLTableCellElement>('.cm-markra-table td')[1];
    expect(cell?.textContent).toBe('⌘B');
    cell.dispatchEvent(new MouseEvent('click', { bubbles: true, detail: 1 }));
    expect(document.activeElement).toBe(cell);
    expect(cell.contains(document.getSelection()!.anchorNode)).toBe(true);
  } finally {
    view.destroy(); root.remove(); document.documentElement.toggleAttribute('data-mobile', mobile);
  }
});

it.each([
  ['上方留白', 'wrapper', 85, '上文\n署名\n%s\n\n下文'],
  ['下方留白', 'wrapper', 210, '上文\n\n%s\n\n署名下文'],
  ['同行留白', 'line', 185, '上文\n\n%s\n\n署名下文'],
] as const)('routes a mobile tap at %s to editable prose outside the table', (_, target, clientY, expected) => {
  const markdown = '| 功能 | 快捷键 |\n| --- | --- |\n| 粗体 | ⌘B |';
  const source = `上文\n\n${markdown}\n\n下文`;
  const root = document.body.appendChild(document.createElement('div'));
  const mobile = document.documentElement.hasAttribute('data-mobile');
  document.documentElement.setAttribute('data-mobile', '');
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(source, 'table-touch-edge');
    const wrapper = root.querySelector('.qingyu-table-controls')!;
    wrapper.querySelector('table')!.getBoundingClientRect = () => new DOMRect(20, 100, 200, 100);
    view.dispatch({ selection: { anchor: source.indexOf('|') } });
    (target === 'wrapper' ? wrapper : wrapper.closest('.cm-line')!).dispatchEvent(new MouseEvent('click', { clientY, bubbles: true, cancelable: true, detail: 1 }));
    view.dispatch(view.state.replaceSelection('署名'));
    expect(view.state.doc.toString()).toBe(expected.replace('%s', markdown));
    expect(root.querySelectorAll('.cm-markra-table td')).toHaveLength(2);
  } finally {
    view.destroy(); root.remove(); document.documentElement.toggleAttribute('data-mobile', mobile);
  }
});

it('keeps a tapped cell focused when WebKit moves the table before delivering click', () => {
  const source = '| A | B |\n| --- | --- |\n| Paint | 12 |\n\n```js\nconst amount = 12;\n```';
  const root = document.body.appendChild(document.createElement('div'));
  const mobile = document.documentElement.hasAttribute('data-mobile');
  document.documentElement.setAttribute('data-mobile', '');
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(source, 'table-cell-tap-viewport-shift');
    const grid = root.querySelector('.cm-markra-table')!;
    const cell = grid.querySelector('td')!;
    let top = 100;
    grid.getBoundingClientRect = () => new DOMRect(20, top, 200, 100);
    const tap = { clientX: 80, clientY: 175, detail: 1, bubbles: true, cancelable: true };
    cell.dispatchEvent(new MouseEvent('mousedown', tap));
    placeTableCellCaret(cell, 2);
    top -= 67;
    grid.parentElement!.dispatchEvent(new MouseEvent('click', tap));
    expect(document.activeElement?.closest('.cm-markra-table')).toBe(grid);
    expect(cell.contains(document.getSelection()!.anchorNode)).toBe(true);
    expect(document.getSelection()!.anchorOffset).toBe(2);
    expect(view.state.doc.toString()).toBe(source);
  } finally {
    view.destroy(); root.remove(); document.documentElement.toggleAttribute('data-mobile', mobile);
  }
});

it('inserts a link at the selected cell text and lets the URL be typed in place', async () => {
  const source = '| A | B |\n| --- | --- |\n| **前文** 资料 | 12 |\n\n下文';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(source, 'table-cell-link');
    let cell = root.querySelector('td')!;
    placeTableCellCaret(cell, 3);
    const selection = getSelection()!;
    selection.extend(selection.anchorNode!, selection.anchorOffset + 2);
    editor.format('link');
    await Promise.resolve();
    const draft = source.replace('资料', '[资料](https://)');
    expect(view.state.doc.toString()).toBe(draft);
    cell = root.querySelector('td')!;
    expect(cell.textContent).toBe('**前文** [资料](https://)');
    expect(selection.anchorNode?.textContent?.slice(0, selection.anchorOffset)).toBe('**前文** [资料](https://');
    const url = selection.getRangeAt(0);
    url.insertNode(document.createTextNode('example.com'));
    cell.closest('table')!.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText' }));
    expect(view.state.doc.toString()).toBe(draft.replace('https://', 'https://example.com'));
  } finally { view.destroy(); root.remove(); }
});

it('opens a table link from a read-only note without revealing the source', () => {
  const source = '| 标签 |\n| --- |\n| [详情](https://example.com) |';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {}, { readOnly: true });
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const open = vi.spyOn(window, 'open').mockReturnValue(null);
  try {
    editor.activate(source, 'readonly-table-link');
    const link = root.querySelector<HTMLAnchorElement>('.cm-markra-table a')!;
    link.dispatchEvent(new MouseEvent('mousedown', { button: 0, bubbles: true, cancelable: true }));
    expect(open).toHaveBeenCalledWith('https://example.com', '_blank', 'noopener,noreferrer');
    expect(link.isConnected).toBe(true);
    expect(link.classList.contains('cm-markra-link')).toBe(true);
    link.click();
    expect(open).toHaveBeenCalledTimes(2);
    link.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    expect(open).toHaveBeenCalledTimes(3);
    expect(view.state.doc.toString()).toBe(source);
  } finally { open.mockRestore(); view.destroy(); root.remove(); }
});

it('reuses the writing editor for read-only notes and restores editing without carrying undo history', () => {
  const root = document.body.appendChild(document.createElement('div'));
  const onChange = vi.fn();
  const editor = createRichEditor(root, onChange);
  const { view } = editor;
  try {
    editor.activate('正文', 'note');
    editor.insert('新增');
    expect(onChange).toHaveBeenCalledOnce();
    expect(undoDepth(view.state)).toBe(1);

    editor.activate('| 字段 |\n| --- |\n| 内容 |', 'note', undefined, undefined, true);
    expect(view.state.readOnly).toBe(true);
    expect(root.querySelector('table')?.textContent).toContain('内容');
    expect(root.querySelector('[contenteditable="true"]')).toBeNull();
    editor.insert('不能写入');
    expect(onChange).toHaveBeenCalledOnce();

    editor.activate('另一篇', 'other');
    expect(view.state.readOnly).toBe(false);
    expect(view.state.doc.toString()).toBe('另一篇');
    expect(undoDepth(view.state)).toBe(0);
    expect(root.querySelectorAll('.cm-editor')).toHaveLength(1);
  } finally { view.destroy(); root.remove(); }
});

it('places the cell caret outside the rendered link so typing can continue', () => {
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate('| 标签 |\n| --- |\n| [详情](https://example.com) |', 'table-link-caret');
    const cell = root.querySelector<HTMLTableCellElement>('td')!;
    placeTableCellCaret(cell, 2);
    const caret = getSelection()!;
    expect(caret.anchorNode).toBe(cell);
    expect(cell.childNodes[caret.anchorOffset - 1]).toBe(cell.querySelector('a'));
  } finally { view.destroy(); root.remove(); }
});

it('toggles a Pinto checklist column while preserving the cell values and adjacent types', async () => {
  const plain = '<!-- pane-table: 状态=text; 备注=number -->\n| 状态 | 备注 |\n| --- | ---: |\n|  | 12 |\n| Yes | 0 |\n| 待确认 | 3 |';
  const typed = plain.replace('状态=text', '状态=choice Yes|No');
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const grid = () => root.querySelector('.cm-markra-table')!;
  const checks = () => [...grid().querySelectorAll<HTMLButtonElement>('[role=checkbox]')];
  try {
    editor.activate(plain, 'pinto-typed-column');
    placeTableCellCaret(grid().querySelector('td')!, 0);
    editor.format('task');
    await Promise.resolve();
    expect(view.state.doc.toString()).toBe(typed);
    expect(document.activeElement?.closest('th')?.textContent).toBe('状态');
    expect(checks().map(button => button.ariaChecked)).toEqual(['false', 'true', 'false']);
    expect(checks()[2]?.textContent).toBe('待确认');
    checks()[0]!.click();
    expect(view.state.doc.toString()).toBe(typed.replace('|  | 12 |', '| Yes | 12 |'));
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(typed);
    editor.activate(typed, 'reopened-pinto-typed-column');
    expect(checks().map(button => button.ariaChecked)).toEqual(['false', 'true', 'false']);
    placeTableCellCaret(grid().querySelector('th')!, 0);
    editor.format('task');
    await Promise.resolve();
    expect(view.state.doc.toString()).toBe(plain);
    expect(checks()).toHaveLength(0);
  } finally { view.destroy(); root.remove(); }
});

it('moves Pinto column types with inserted and deleted columns', async () => {
  const source = '| 项目 | 状态 | 金额 |\n| --- | --- | ---: |\n| 收据 | Yes | 12 |';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const grid = () => root.querySelector<HTMLTableElement>('.cm-markra-table')!;
  try {
    editor.activate(source, 'pinto-checklist-columns');
    placeTableCellCaret(grid().rows[0]!.cells[1]!, 0);
    editor.format('task');
    await Promise.resolve();
    const typed = '<!-- pane-table: 项目=text; 状态=choice Yes|No; 金额=text -->\n' + source;
    expect(view.state.doc.toString()).toBe(typed);
    expect(grid().rows[1]!.cells[1]!.querySelector('[role=checkbox]')?.getAttribute('aria-checked')).toBe('true');
    placeTableCellCaret(grid().rows[1]!.cells[0]!, 0);
    grid().rows[1]!.cells[0]!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }));
    expect(document.activeElement?.closest('td')?.textContent).toBe('12');
    placeTableCellCaret(grid().rows[0]!.cells[0]!, 0);
    editor.captureTableMenu(root)?.();
    document.querySelector<HTMLButtonElement>('button[aria-label="在左侧插入列"]')!.click();
    await Promise.resolve();
    await Promise.resolve();
    expect(grid().rows[1]!.cells[2]!.querySelector('[role=checkbox]')).not.toBeNull();
    placeTableCellCaret(grid().rows[0]!.cells[0]!, 0);
    editor.captureTableMenu(root)?.();
    document.querySelector<HTMLButtonElement>('button[aria-label="删除列"]')!.click();
    await Promise.resolve();
    await Promise.resolve();
    expect(view.state.doc.toString()).toBe(typed);
    expect(undo(view)).toBe(true);
    expect(undo(view)).toBe(true);
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(source);
  } finally { view.destroy(); root.remove(); }
});

it.each([
  ['表格右侧', 'line', 300, 130],
  ['添加列按钮旁', 'wrapper', 225, 185],
  ['表格下方', 'wrapper', 100, 210],
] as const)('keeps the desktop grid visible when clicking %s and types after it', (_, target, clientX, clientY) => {
  const markdown = '| Date | Distance km |\n| --- | ---: |\n| 2026-09-22 | 5 |';
  const source = `上文\n\n${markdown}\n\n下文`;
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(source, 'desktop-table-edge');
    const wrapper = root.querySelector('.qingyu-table-controls')!;
    wrapper.querySelector('table')!.getBoundingClientRect = () => new DOMRect(20, 100, 200, 100);
    wrapper.querySelector('.markra-table-scroll')!.getBoundingClientRect = () => new DOMRect(19, 99, 202, 102);
    (target === 'wrapper' ? wrapper : wrapper.closest('.cm-line')!).dispatchEvent(new MouseEvent('mousedown', {
      clientX, clientY, bubbles: true, cancelable: true, detail: 1,
    }));
    expect(view.state.doc.toString()).toBe(source);
    const right = clientY < 200;
    expect(view.state.selection.main.head).toBe(source.indexOf('下文') - Number(right));
    expect(root.querySelector('.cm-markra-table')).not.toBeNull();
    expect(Boolean(root.querySelector('[data-table-edge-caret]'))).toBe(right);
    view.dispatch(view.state.replaceSelection('署名'), { userEvent: 'input.type' });
    expect(view.state.doc.toString()).toBe(source.replace('下文', right ? '署名\n\n下文' : '署名下文'));
    expect(root.querySelector('[data-table-edge-caret]')).toBeNull();
  } finally { view.destroy(); root.remove(); }
});

it.each([
  ['', ''],
  ['\n', ''],
  ['\n\n', ''],
  ['\n\n| A | B |\n| --- | --- |\n| 1 | 2 |', '\n\n| A | B |\n| --- | --- |\n| 1 | 2 |'],
  ['\n```js\nconsole.log(1);\n```', '\n\n```js\nconsole.log(1);\n```'],
  ['\n\n```js\nconsole.log(1);\n```', '\n\n```js\nconsole.log(1);\n```'],
] as const)('keeps the note unchanged until typing after the grid before %j', (next, remainder) => {
  const markdown = '| 功能 | 快捷键 |\n| --- | --- |\n| 粗体 | ⌘B |';
  const root = document.body.appendChild(document.createElement('div'));
  const change = vi.fn();
  const editor = createRichEditor(root, change);
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(markdown + next, 'table-touch-edge-end');
    const wrapper = root.querySelector('.qingyu-table-controls')!;
    wrapper.querySelector('table')!.getBoundingClientRect = () => new DOMRect(20, 100, 200, 100);
    wrapper.dispatchEvent(new MouseEvent('mousedown', { clientY: 210, bubbles: true, cancelable: true, detail: 1 }));
    expect(view.state.doc.toString()).toBe(markdown + next);
    expect(change).not.toHaveBeenCalled();
    const input = root.querySelector<HTMLTextAreaElement>('.markra-table-edge-input')!;
    if (next !== '\n\n') {
      expect(document.activeElement).toBe(input);
      expect(root.querySelector('[data-table-edge-caret=right]')).not.toBeNull();
    }
    if (next.startsWith('\n```')) expect(view.state.selection.main.head).toBe(markdown.length);
    if (next.includes('```')) expect(root.textContent).not.toContain('```');
    if (!next) {
      view.dispatch({ changes: { from: markdown.length - 1, to: markdown.length }, userEvent: 'delete.backward' });
      expect(view.state.doc.toString()).toBe(markdown);
    }
    if (input) {
      input.value = '2';
      input.dispatchEvent(new InputEvent('input', { data: '2', inputType: 'insertText', bubbles: true }));
    } else view.dispatch(view.state.replaceSelection('2'), { userEvent: 'input.type' });
    expect(view.state.doc.toString()).toBe(`${markdown}\n\n2${remainder}`);
    expect(root.querySelectorAll('.cm-markra-table')).toHaveLength(next.startsWith('\n\n|') ? 2 : 1);
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(markdown + next);
  } finally { view.destroy(); root.remove(); }
});

it.each([
  ['\n', '.cm-markra-code-header-line'],
  ['\n\n', '.cm-markra-code-header-line'],
  ['\n\n', '.cm-markra-table-hidden-line + .cm-markra-empty-line'],
] as const)('keeps the code preview closed when clicking the edge after a table (%j, %s)', async (separator, target) => {
  const table = '| 名称 | 状态 |\n| --- | --- |\n| 表格 | 完成 |';
  const code = '```js\nconsole.log(12);\n```';
  const source = table + separator + code;
  const root = document.body.appendChild(document.createElement('div'));
  const onChange = vi.fn();
  const editor = createRichEditor(root, onChange);
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const clickEdge = () => {
    const event = new MouseEvent('mousedown', { button: 0, detail: 1, bubbles: true, cancelable: true });
    root.querySelector(target)!.dispatchEvent(event);
    return event.defaultPrevented;
  };

  try {
    editor.activate(source, 'table-to-code-hit-target');
    expect(clickEdge()).toBe(true);
    expect(root.querySelector('.cm-markra-table')).not.toBeNull();
    expect(view.state.selection.main.head).toBe(table.length + (separator.length - 1));
    expect(view.state.doc.toString()).toBe(source);
    expect(onChange).not.toHaveBeenCalled();
    expect(root.textContent).not.toContain('```');

    const input = root.querySelector<HTMLTextAreaElement>('.markra-table-edge-input')!;
    expect(document.activeElement).toBe(input);
    input.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }));
    input.value = 'xin';
    input.dispatchEvent(new InputEvent('input', { inputType: 'insertCompositionText', isComposing: true, bubbles: true }));
    expect(view.state.doc.toString()).toBe(source);
    input.dispatchEvent(new CompositionEvent('compositionend', { data: '', bubbles: true }));
    expect(input.value).toBe('');
    expect(view.state.doc.toString()).toBe(source);

    input.dispatchEvent(new CompositionEvent('compositionstart', { bubbles: true }));
    input.dispatchEvent(new CompositionEvent('compositionend', { data: '新段落', bubbles: true }));
    expect(view.state.doc.toString()).toBe(`${table}\n\n新段落\n\n${code}`);
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(source);
    if (separator === '\n\n') {
      await Promise.resolve();
      expect(document.activeElement?.classList.contains('markra-table-edge-input')).toBe(true);
    }

    expect(clickEdge()).toBe(true);
    expect(root.querySelector('.cm-markra-table')).not.toBeNull();
    const nextInput = root.querySelector<HTMLTextAreaElement>('.markra-table-edge-input')!;
    nextInput.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    expect(view.state.doc.toString()).toBe(`${table}\n\n\n${code}`);
    expect(view.state.selection.main.head).toBe(table.length + 2);
    expect(deleteCharBackward(view)).toBe(true);
    await Promise.resolve();
    expect(view.state.doc.toString()).toBe(`${table}\n\n${code}`);
    expect(document.activeElement?.classList.contains('markra-table-edge-input')).toBe(true);
    expect(root.textContent).not.toContain('```');
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(`${table}\n\n\n${code}`);
    view.dispatch(view.state.replaceSelection('回车后输入'), { userEvent: 'input.type' });
    expect(view.state.doc.toString()).toBe(`${table}\n\n回车后输入\n${code}`);
    expect(root.querySelector('.cm-markra-table')).not.toBeNull();
    expect(root.textContent).not.toContain('```');
    expect(undo(view)).toBe(true);
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(source);

    view.focus();
    view.dispatch({ selection: { anchor: source.indexOf('console') + 3 } });
    expect(root.textContent).toContain('```js');
    expect(root.querySelector('[data-code-block-start]')).toBeNull();
  } finally { view.destroy(); root.remove(); }
});

it.each([
  ['insertLineBreak', '\n'],
  ['insertParagraph', '\n\n'],
] as const)('handles software keyboard %s at a table edge as one Return', (inputType, separator) => {
  const table = '| A | B |\n| --- | --- |\n| 1 | 2 |';
  const code = '```js\nconsole.log(12);\n```';
  const root = document.body.appendChild(document.createElement('div'));
  const wasMobile = document.documentElement.hasAttribute('data-mobile');
  document.documentElement.setAttribute('data-mobile', '');
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(table + separator + code, 'table-edge-software-return');
    const grid = root.querySelector('.cm-markra-table')!;
    const line = grid.closest('.cm-line')!;
    let top = 100;
    grid.getBoundingClientRect = () => new DOMRect(20, top, 200, 100);
    const tap = { clientX: 225, clientY: 175, detail: 1, bubbles: true, cancelable: true };
    line.dispatchEvent(new MouseEvent('mousedown', tap));
    // iOS can move the table when editor focus shows the keyboard controls before click.
    top += 67;
    line.dispatchEvent(new MouseEvent('click', tap));
    expect(view.state.doc.toString()).toBe(table + separator + code);
    const input = root.querySelector<HTMLTextAreaElement>('.markra-table-edge-input')!;
    expect(document.activeElement).toBe(input);
    input.value = '\n';
    input.dispatchEvent(new InputEvent('input', { inputType, bubbles: true }));
    expect(view.state.doc.toString()).toBe(table + '\n\n\n' + code);
    expect(view.state.selection.main.head).toBe(table.length + 2);
    expect(root.querySelector('.cm-markra-table')).not.toBeNull();
    expect(root.textContent).not.toContain('```');
  } finally {
    view.destroy(); root.remove(); document.documentElement.toggleAttribute('data-mobile', wasMobile);
  }
});

it.each([
  ['\n\n', '```js\nconsole.log(12);\n```'],
  ['\n \n', '```js\nconsole.log(12);\n```'],
  ['\n\t\n', '```js\nconsole.log(12);\n```'],
  ['\n\n', '## 下文'],
  ['\n\n', '这段正文应独立'],
] as const)('inserts one editable row on Return at the right edge with %j before %j', async (separator, following) => {
  const table = '| 名称 | 状态 |\n| --- | --- |\n| 表格 | 完成 |';
  const source = table + separator + following;
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const press = (key: string) => document.activeElement!.dispatchEvent(
    new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }),
  );
  try {
    editor.activate(source, 'table-right-edge-one-return');
    const wrapper = root.querySelector('.qingyu-table-controls')!;
    wrapper.querySelector('table')!.getBoundingClientRect = () => new DOMRect(20, 100, 200, 100);
    wrapper.querySelector('.markra-table-scroll')!.getBoundingClientRect = () => new DOMRect(19, 99, 202, 102);
    wrapper.closest('.cm-line')!.dispatchEvent(new MouseEvent('mousedown', {
      clientX: 300, clientY: 170, bubbles: true, cancelable: true, detail: 1,
    }));
    expect(view.state.doc.toString()).toBe(source);
    expect(document.activeElement?.classList.contains('markra-table-edge-input')).toBe(true);
    press('Enter');
    expect(view.state.doc.toString()).toBe(table + separator + '\n' + following);
    expect(root.querySelector('.cm-markra-table')).not.toBeNull();
    const structural = root.querySelector('.cm-markra-table-hidden-line + .cm-markra-table-separator');
    expect(structural?.nextElementSibling?.classList.contains('cm-markra-empty-line')).toBe(true);
    expect(view.state.selection.main.head).toBe(table.length + separator.length);
    press('Backspace');
    await Promise.resolve();
    expect(view.state.doc.toString()).toBe(source);
    expect(document.activeElement?.classList.contains('markra-table-edge-input')).toBe(true);
    expect(root.textContent).not.toContain('```');
  } finally { view.destroy(); root.remove(); }
});

it('marks the table on the first Backspace after it and deletes it on the second, like Pinto', async () => {
  const table = '| 名称 | 状态 |\n| --- | --- |\n| 表格 | 完成 |';
  const source = '前文\n\n' + table + '\n\n后文';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const press = (key: string) => document.activeElement!.dispatchEvent(
    new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }),
  );
  try {
    editor.activate(source, 'table-backspace-delete');
    const wrapper = root.querySelector('.qingyu-table-controls')!;
    wrapper.querySelector('table')!.getBoundingClientRect = () => new DOMRect(20, 100, 200, 100);
    wrapper.querySelector('.markra-table-scroll')!.getBoundingClientRect = () => new DOMRect(19, 99, 202, 102);
    wrapper.closest('.cm-line')!.dispatchEvent(new MouseEvent('mousedown', {
      clientX: 300, clientY: 170, bubbles: true, cancelable: true, detail: 1,
    }));
    expect(document.activeElement?.classList.contains('markra-table-edge-input')).toBe(true);
    press('Backspace');
    expect(view.state.doc.toString()).toBe(source);
    expect(wrapper.querySelector('.markra-table-scroll')!.hasAttribute('data-table-armed')).toBe(true);
    // 软键盘只发 beforeinput 时也一样删除。
    await new Promise(resolve => setTimeout(resolve));
    document.activeElement!.dispatchEvent(new InputEvent('beforeinput', { inputType: 'deleteContentBackward', bubbles: true, cancelable: true }));
    expect(view.state.doc.toString()).toBe('前文\n\n\n后文');
    expect(root.querySelector('.cm-markra-table')).toBeNull();
    // 撤销后回到删除前的右边缘光标，而不是进入最后一个单元格。
    expect(undo(view)).toBe(true);
    await Promise.resolve();
    expect(view.state.doc.toString()).toBe(source);
    expect(document.activeElement?.classList.contains('markra-table-edge-input')).toBe(true);
    expect(root.querySelector('[data-table-edge-caret=right]')).not.toBeNull();
    expect(root.querySelector('[data-table-armed]')).toBeNull();
  } finally { view.destroy(); root.remove(); }
});

it('keeps the table separator compact across consecutive Returns and Backspaces before a code fence', async () => {
  const table = '| 名称 | 状态 |\n| --- | --- |\n| 表格 | 完成 |';
  const code = '```js\nconsole.log(12);\n```';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const press = (key: string) => document.activeElement!.dispatchEvent(
    new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }),
  );
  const check = (blankCount: number) => {
    expect(view.state.doc.toString()).toBe(table + '\n'.repeat(blankCount + 1) + code);
    const blanks = [...root.querySelectorAll('.cm-markra-empty-line')];
    expect(blanks).toHaveLength(blankCount);
    expect(blanks.filter(line => line.classList.contains('cm-markra-table-separator'))).toEqual(blanks.slice(0, 1));
    expect(root.textContent).not.toContain('```');
  };
  try {
    editor.activate(table + '\n\n' + code, 'table-to-code-consecutive-returns');
    root.querySelector('.cm-markra-code-header-line')!.dispatchEvent(
      new MouseEvent('mousedown', { detail: 1, bubbles: true, cancelable: true }),
    );
    for (let blankCount = 2; blankCount <= 4; blankCount++) {
      press('Enter');
      check(blankCount);
      expect(view.state.selection.main.head).toBe(table.length + blankCount);
    }
    for (let blankCount = 3; blankCount >= 1; blankCount--) {
      press('Backspace');
      check(blankCount);
      expect(view.state.selection.main.head).toBe(table.length + blankCount);
    }
    await Promise.resolve();
    expect(document.activeElement?.classList.contains('markra-table-edge-input')).toBe(true);
  } finally { view.destroy(); root.remove(); }
});

it.each(['快捷键', '编辑菜单'])('keeps the table separator compact when typing and accepts undo from its edge via %s', action => {
  const table = '| 名称 | 状态 |\n| --- | --- |\n| 表格 | 完成 |';
  const code = '```js\nconsole.log(12);\n```';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const separator = () => root.querySelector('.cm-markra-table-separator');
  const offset = table.length + 2;
    const text = '**新的正文**';
  try {
    editor.activate(table + '\n\n\n' + code, 'table-separator-stable');
    view.focus();
    view.dispatch({ selection: { anchor: offset } });
    expect(separator()).not.toBeNull();
    view.dispatch(view.state.replaceSelection(text), { userEvent: 'input.type' });
    const edited = table + '\n\n' + text + '\n' + code;
    expect(view.state.doc.toString()).toBe(edited);
    expect(separator()?.nextElementSibling?.textContent).toBe('新的正文');
    view.dispatch({ selection: { anchor: offset } });
    view.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { key: 'Backspace', bubbles: true, cancelable: true }));
    expect(view.state.doc.toString()).toBe(edited);
    const edge = root.querySelector('.markra-table-edge-input')!;
    expect(document.activeElement).toBe(edge);
    expect(root.querySelector('table')?.rows).toHaveLength(2);
    if (action === '快捷键') {
      edge.dispatchEvent(new KeyboardEvent('keydown', { key: 'z', metaKey: true, bubbles: true, cancelable: true }));
    } else runEditorHistoryMenu('undo');
    expect(view.state.doc.toString()).toBe(table + '\n\n\n' + code);
  } finally { view.destroy(); root.remove(); }
});

it.each([
  ['继续引用', '> A quote', 9, ['Enter'], '> A quote\n> ', 12],
  ['退出空引用', '> A quote', 9, ['Enter', 'Enter'], '> A quote\n', 10],
  ['删除引用标记', '> A quote', 2, ['Backspace'], 'A quote', 0],
  ['移到上一行', '> A quote', 2, ['ArrowLeft'], '> A quote', -1],
  ['跳过引用标记', '> A quote', -1, ['ArrowRight'], '> A quote', 2],
  ['代码里的大于号', '```md\n> \n```', 8, ['Enter'], '```md\n> \n\n```', 9],
] as const)('matches Pinto when editing a block quote / %s', (_, line, caret, keys, expected, destination) => {
  const before = '# 标题\n\n上文\n\n';
  const after = '\n\n下文';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(before + line + after, 'quote-boundary');
    view.focus();
    view.dispatch({ selection: { anchor: before.length + caret } });
    for (const key of keys) view.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
    expect(view.state.doc.toString()).toBe(before + expected + after);
    expect(view.state.selection.main.head).toBe(before.length + destination);
  } finally { view.destroy(); root.remove(); }
});

it('renders body text without a quote bar after leaving an empty quote', () => {
  const source = '# 标题\n\n> 引用';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(source, 'quote-to-body');
    view.focus();
    view.dispatch({ selection: { anchor: source.length } });
    for (let i = 0; i < 2; i++) view.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    view.dispatch(view.state.replaceSelection('正文'), { userEvent: 'input.type' });
    expect(view.state.doc.toString()).toBe(source + '\n正文');
    expect(root.querySelectorAll('.cm-markra-blockquote')).toHaveLength(1);
    expect(root.querySelector('.cm-markra-blockquote')?.textContent).toBe('引用');
  } finally { view.destroy(); root.remove(); }
});

it.each([
  ['- |事项', 'Backspace', '|事项'],
  ['32. |编号', 'Backspace', '|编号'],
  ['- [ ] |待办', 'Backspace', '|待办'],
  ['+ [X] |已完成', 'Backspace', '|已完成'],
  ['- 上一项\n- [x] |已完成\n- [ ] 下一项', 'Backspace', '- 上一项\n|已完成\n- [ ] 下一项'],
  ['- 上级\n  - [ ] |子任务', 'Backspace', '- 上级\n- [ ] |子任务'],
  ['\n- |事项', 'ArrowLeft', '|\n- 事项'],
  ['\n- [x] |已完成', 'ArrowLeft', '|\n- [x] 已完成'],
  ['|\n- [ ] 待办', 'ArrowRight', '\n- [ ] |待办'],
  ['```md\n- [ ] |原样保留\n```', 'Backspace', '```md\n- [ ]|原样保留\n```'],
] as const)('matches Pinto at the start of a list item / %s / %s', (marked, key, expected) => {
  const before = '# 标题\n\n上文\n\n';
  const after = '\n\n下文';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const source = before + marked.replace('|', '') + after;
  try {
    editor.activate(source, 'list-marker-boundary');
    view.focus();
    view.dispatch({ selection: { anchor: before.length + marked.indexOf('|') } });
    view.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
    expect(view.state.doc.toString()).toBe(before + expected.replace('|', '') + after);
    expect(view.state.selection.main.head).toBe(before.length + expected.indexOf('|'));
    if (key === 'Backspace') {
      expect(undo(view)).toBe(true);
      expect(view.state.doc.toString()).toBe(source);
    }
  } finally { view.destroy(); root.remove(); }
});

it('opens Pinto’s callout types from the leading icon and preserves the title and fold marker', async () => {
  const source = '# 上文\n\n> [!FAQ]- 自定义标题\n> 保留 **正文**\n\n```md\n> [!BUG] 源码示例\n```';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const icon = () => root.querySelector<HTMLButtonElement>('button[aria-label="更改标注类型"]')!;
  try {
    editor.activate(source, 'typed-callout');
    expect(root.querySelectorAll('.markra-callout-header')).toHaveLength(1);
    expect(root.querySelector('.markra-callout-title')?.textContent?.trim()).toBe('自定义标题');
    icon().click();
    const menu = document.querySelector('[role=menu][aria-label="标注类型"]')!;
    expect([...menu.querySelectorAll('[role=menuitemradio]')].map(item => item.ariaLabel)).toEqual([
      'Note', 'Info', 'Tip', 'Success', 'Question', 'Warning', 'Danger', 'Bug', 'Example', 'Quote',
    ]);
    expect(menu.querySelector('[aria-checked=true]')?.ariaLabel).toBe('Question');
    menu.querySelector<HTMLButtonElement>('button[aria-label=Quote]')!.click();
    await Promise.resolve();
    expect(view.state.doc.toString()).toBe(source.replace('FAQ', 'quote'));
    expect(root.querySelector('.cm-markra-callout')?.getAttribute('data-callout-tone')).toBe('quote');
    expect(root.textContent).not.toContain('# 上文');
    expect(document.activeElement).toBe(icon());
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(source);
  } finally { view.destroy(); root.remove(); }
});

it('moves checked checklist items down after the last click, keeping focus, the caret and one undo step per click', async () => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'Date'] });
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const checkbox = (index: number) => root.querySelectorAll<HTMLInputElement>('.cm-markra-task-checkbox')[index];
  const click = (index: number) => { const task = checkbox(index); task.focus(); task.click(); };
  const source = '# 清单\n\n- [ ] 一 🍵\n- [ ] 二\n- [ ] 三\n- [x] 四\n\n- [ ] 另一组';
  const afterFirst = source.replace('[ ] 一', '[x] 一');
  const afterSecond = afterFirst.replace('[ ] 二', '[x] 二');
  const sorted = source.replace('- [ ] 一 🍵\n- [ ] 二\n- [ ] 三', '- [ ] 三\n- [x] 一 🍵\n- [x] 二');
  try {
    editor.activate(source, 'checklist-sort');
    click(0);
    expect(document.activeElement).toBe(checkbox(0));
    vi.advanceTimersByTime(300);
    expect(view.state.doc.toString()).toBe(afterFirst);
    click(1);
    view.dispatch({ selection: { anchor: source.indexOf('🍵') + 2 } });
    vi.advanceTimersByTime(599);
    expect(view.state.doc.toString()).toBe(afterSecond);
    vi.advanceTimersByTime(1);
    expect(view.state.doc.toString()).toBe(sorted);
    expect(document.activeElement).toBe(checkbox(2));
    expect(view.state.selection.main.head).toBe(sorted.indexOf('🍵') + 2);
    expect(undoDepth(view.state)).toBe(2);
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(afterFirst);
    await Promise.resolve();
    expect(document.activeElement).toBe(checkbox(2));
    expect(redo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(sorted);
  } finally { view.destroy(); root.remove(); vi.useRealTimers(); }
});

it.each(['preference', 'undo', 'edit', 'switch', 'nested', 'wrapped text', 'same item'] as const)(
  'keeps the checklist in place when %s interrupts sorting', (scenario) => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'Date'] });
    const root = document.body.appendChild(document.createElement('div'));
    let enabled = true;
    const editor = createRichEditor(root, () => {}, { sortCheckedItems: () => enabled });
    const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
    const click = () => root.querySelector<HTMLInputElement>('.cm-markra-task-checkbox')!.click();
    const extra = scenario === 'nested' ? '\n  - [ ] 子项' : scenario === 'wrapped text' ? '\n  补充说明' : '';
    const source = `- [ ] 一\n- [ ] 二${extra}`;
    try {
      editor.activate(source, 'checklist-sort-first');
      click();
      let expected = source.replace('[ ] 一', '[x] 一');
      if (scenario === 'preference') enabled = false;
      if (scenario === 'undo') { undo(view); expected = source; }
      if (scenario === 'edit') {
        view.dispatch({ changes: { from: view.state.doc.length, insert: '改' }, userEvent: 'input.type' });
        expected += '改';
      }
      if (scenario === 'switch') { editor.deactivate(); editor.activate(source, 'checklist-sort-second'); expected = source; }
      if (scenario === 'same item') { click(); expected = source; }
      vi.advanceTimersByTime(650);
      expect(view.state.doc.toString()).toBe(expected);
    } finally { view.destroy(); root.remove(); vi.useRealTimers(); }
  },
);

it('toggles a touched task without giving the mobile editor keyboard focus', () => {
  const root = document.body.appendChild(document.createElement('div'));
  const html = document.documentElement;
  const mobile = html.hasAttribute('data-mobile');
  html.setAttribute('data-mobile', '');
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate('# 清单\n\n- [ ] 待办', 'checklist-touch');
    const task = root.querySelector<HTMLInputElement>('.cm-markra-task-checkbox')!;
    const press = new MouseEvent('mousedown', { button: 0, bubbles: true, cancelable: true });
    expect(task.dispatchEvent(press)).toBe(false);
    task.click();
    expect(view.state.doc.toString()).toBe('# 清单\n\n- [x] 待办');
    expect(document.activeElement).toBe(document.body);
  } finally { view.destroy(); root.remove(); html.toggleAttribute('data-mobile', mobile); }
});

it('unchecks only the checklist under the pointer and undoes it in one step', async () => {
  const original = '# 清单\n\n- [ ] 待办\n- [x] 任务\n  - [X] 子任务\n\n- [x] 另一组\n\n```md\n- [x] 示例\n```';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(original, 'checklist-menu');
    const checkbox = root.querySelector<HTMLInputElement>('.cm-markra-task-checkbox')!;
    expect(checkbox.checked).toBe(false);
    checkbox.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    expect(checkbox.dispatchEvent(new MouseEvent('auxclick', { button: 2, bubbles: true, cancelable: true }))).toBe(false);
    expect(view.state.doc.toString()).toBe(original);
    document.querySelector<HTMLButtonElement>('button[aria-label="取消勾选此清单的所有任务"]')!.click();
    await Promise.resolve();
    expect(view.state.doc.toString()).toBe(original.replace('[x] 任务', '[ ] 任务').replace('[X] 子任务', '[ ] 子任务'));
    const notice = root.querySelector('.editor-history-notice')!;
    expect(notice.querySelector('[role="status"]')?.textContent).toBe('已取消勾选 2 项');
    notice.querySelector('button')!.click();
    expect(view.state.doc.toString()).toBe(original);
    expect(notice.isConnected).toBe(false);
    view.dispatch({ selection: { anchor: original.indexOf('示例') } });
    expect(editor.canUntickChecklist).toBe(false);
  } finally {
    view.destroy();
    root.remove();
  }
});

it('refreshes missing-note underlines without replacing a table or moving its caret', () => {
  const source = '# 笔记\n\n正文 [[待建笔记|别名]] 与 [[已有笔记]]\n\n| A | B |\n| --- | --- |\n| [[待建笔记]] | 正在编辑 |';
  const root = document.body.appendChild(document.createElement('div'));
  const onChange = vi.fn();
  const available = new Set(['已有笔记']);
  const editor = createRichEditor(root, onChange, { noteLinkExists: target => available.has(target) });
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const missing = () => [...root.querySelectorAll('a[data-missing-note]')].map(link => link.textContent);
  try {
    editor.activate(source, 'refresh-wiki-status');
    expect(missing()).toEqual(['别名', '待建笔记']);
    const table = root.querySelector('table')!;
    const cell = table.rows[1].cells[1];
    placeTableCellCaret(cell, 2);
    const active = document.activeElement;
    const anchor = document.getSelection()!.anchorNode;
    expect(cell.contains(anchor)).toBe(true);
    available.add('待建笔记');
    editor.refreshNoteLinks();
    expect(missing()).toEqual([]);
    expect(root.querySelector('table')).toBe(table);
    expect(document.activeElement).toBe(active);
    expect(document.getSelection()!.anchorNode).toBe(anchor);
    expect(document.getSelection()!.anchorOffset).toBe(2);
    available.delete('已有笔记');
    editor.refreshNoteLinks();
    expect(missing()).toEqual(['已有笔记']);
    expect(view.state.doc.toString()).toBe(source);
    expect(onChange).not.toHaveBeenCalled();
  } finally {
    view.destroy();
    root.remove();
  }
});

it('opens rendered prose links on release and reveals their Markdown only in the address', () => {
  const url = 'https://example.com/long-address';
  const label = '设计文档说明';
  const source = `# 笔记\n\n查看 [${label}](${url}) 后续正文`;
  const root = document.body.appendChild(document.createElement('div'));
  const open = vi.spyOn(window, 'open').mockImplementation(() => null);
  const editor = createRichEditor(root, vi.fn());
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const link = () => root.querySelector<HTMLAnchorElement>(`a[href="${url}"]`)!;
  const pointer = (type: string, clientX = 10, altKey = false) => link().dispatchEvent(new MouseEvent(type, {
    bubbles: true, cancelable: true, button: 0, detail: 1, clientX, altKey,
  }));
  try {
    editor.activate(source, 'prose-link-click');
    view.focus();
    view.dispatch({ selection: { anchor: source.indexOf(label) + 2 } });
    expect(link().textContent).toBe(label);
    expect(root.querySelector('.cm-markra-link-source')).toBeNull();
    pointer('mousedown');
    expect(open).not.toHaveBeenCalled();
    pointer('mouseup', 30);
    expect(open).not.toHaveBeenCalled();
    pointer('mousedown');
    pointer('mouseup');
    pointer('click');
    expect(open).toHaveBeenCalledExactlyOnceWith(url, '_blank', 'noopener,noreferrer');
    pointer('mousedown', 10, true);
    expect(view.state.selection.main.head).toBe(source.indexOf(url));
    expect(root.querySelector('.cm-markra-link-source')).not.toBeNull();
    expect(open).toHaveBeenCalledTimes(1);
    expect(view.state.doc.toString()).toBe(source);
  } finally {
    open.mockRestore();
    view.destroy();
    root.remove();
  }
});

it('restyles standalone links, keeps code and prose links, and can undo a removal', async () => {
  const url = 'https://example.com/docs';
  const original = `# 链接卡片\n\n[文档](${url})\n\n参阅 [文档](${url})。\n\n\`\`\`md\n<${url}>\n\`\`\`\n`;
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const context = (target: Element) => target.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
  const choose = async (name: string) => {
    document.querySelector<HTMLButtonElement>(`button[aria-label="${name}"]`)!.click();
    await Promise.resolve();
  };
  try {
    editor.activate(original, 'links');
    expect(root.querySelectorAll('.qingyu-link-card')).toHaveLength(0);
    const links = root.querySelectorAll(`a[href="${url}"]`);
    expect(links).toHaveLength(2);
    context(links[1]);
    expect(document.querySelector('button[aria-label="显示为卡片"]')).toBeNull();
    context(links[0]);
    await choose('显示为卡片');
    expect(view.state.doc.toString()).toBe(original.replace(`[文档](${url})`, `<${url}>`));
    context(root.querySelector('.qingyu-link-card[data-style=card]')!);
    await choose('紧凑显示');
    expect(view.state.doc.line(3).text).toBe(url);
    context(root.querySelector('.qingyu-link-card[data-style=compact]')!);
    await choose('显示为链接');
    expect(view.state.doc.line(3).text).toBe(`[example.com](${url})`);
    context(root.querySelector(`a[href="${url}"]`)!);
    await choose('显示为卡片');
    const card = root.querySelector('.qingyu-link-card[data-style=card]')!;
    context(card);
    await choose('从笔记中移除');
    expect(view.state.doc.toString()).not.toContain(`\n\n<${url}>`);
    expect(undo(view)).toBe(true);
    expect(view.state.doc.line(3).text).toBe(`<${url}>`);
  } finally {
    view.destroy(); root.remove();
  }
});

it('shows note aliases and summaries and preserves attachment paths and ordinary images', () => {
  const opened: string[] = [];
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {}, {
    openNoteLink: target => opened.push(target),
    describeNoteLink: () => ({ folder: '工作', detail: '第二行摘要' }),
    openAsset: (id, path) => { opened.push(`${id}:${path}`); },
  });
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate('# 测试\n\n![[工作/计划|查看计划]]\n\n![文档](.attachments/id/first%20draft.pdf)\n\n![截图](.attachments/id/img.png)\n', 'cards', 'asset-id');
    const note = root.querySelector<HTMLAnchorElement>('.qingyu-link-card[data-kind=note]')!;
    expect(note.textContent).toContain('查看计划第二行摘要工作');
    note.click();
    root.querySelector<HTMLAnchorElement>('.qingyu-link-card[data-kind=file]')!.click();
    expect(opened).toEqual(['工作/计划', 'asset-id:.attachments/id/first%20draft.pdf']);
    expect(root.querySelectorAll('.qingyu-link-card')).toHaveLength(2);
  } finally {
    view.destroy(); root.remove();
  }
});

it('renders webpage metadata as text and uses its title when the card is changed to a link', async () => {
  const url = 'https://example.com/path/page';
  const original = '# 链接卡片\n\n<' + url + '>\n';
  const info = readWebLinkPreview([
    '<title>备用标题</title>',
    '<meta content="网页 &amp; &lt;文档&gt;" property="og:title">',
    '<meta name="description" content="页面说明">',
    '<meta property="og:image" content="javascript:ignored">',
    '<meta name="twitter:image" content="../cover.png">',
    '<link rel="shortcut icon" href="/mark.png">',
    '<img src="https://unrelated.test/">',
  ].join('\n'), url);
  expect(info).toEqual({
    title: '网页 & <文档>', summary: '页面说明',
    cover: 'https://example.com/cover.png', icon: 'https://example.com/mark.png',
  });
  const root = document.body.appendChild(document.createElement('div'));
  const thumb = vi.fn(async () => null);
  const editor = createRichEditor(root, vi.fn(), { loadLinkPreview: { page: async () => info, thumbnail: thumb } });
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(original, 'web-card-metadata');
    await Promise.resolve();
    expect(root.querySelector('.link-card-title')?.textContent).toBe('网页 & <文档>');
    expect(root.querySelector('.link-card-detail')?.textContent).toBe('页面说明');
    expect(root.querySelector('.qingyu-link-card img')).toBeNull();
    expect(view.state.doc.toString()).toBe(original);
    root.querySelector('.qingyu-link-card')!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    document.querySelector<HTMLButtonElement>('button[aria-label="显示为链接"]')!.click();
    await Promise.resolve();
    expect(view.state.doc.toString()).toBe('# 链接卡片\n\n[网页 \\& \\<文档\\>](' + url + ')\n');
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(original);
  } finally { view.destroy(); root.remove(); }
});

it('opens, resizes and removes a standalone image from its context menu without changing the attachment', async () => {
  const path = '.attachments/folder/a%20b.png';
  const source = `# 图片\n\n![图片\\[说明\\]](${path} "标题")\n\n后文 ![行内图](${path})`;
  const root = document.body.appendChild(document.createElement('div'));
  const openAsset = vi.fn();
  const editor = createRichEditor(root, () => {}, { openAsset });
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const image = () => root.querySelector<HTMLImageElement>('.cm-markra-image-line .cm-markra-image')!;
  const button = (label: string) => document.querySelector<HTMLButtonElement>(`button[aria-label="${label}"]`)!;
  const menu = (target = image()) => target.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
  try {
    editor.activate(source, 'picture-options', 'asset-note');
    image().dispatchEvent(new MouseEvent('mousedown', { button: 2, bubbles: true, cancelable: true }));
    menu();
    expect(root.querySelector('.markra-image-node-source')).toBeNull();
    button('打开图片').click();
    await Promise.resolve();
    expect(openAsset).toHaveBeenCalledExactlyOnceWith('asset-note', path);
    expect(view.state.doc.toString()).toBe(source);

    let current = '适应宽度';
    for (const [label, suffix, size] of [['小图', '|small', 'small'], ['封面', '|cover', 'cover'], ['适应宽度', '', 'fit']]) {
      menu();
      button('图片尺寸').click();
      expect(button(current).getAttribute('aria-checked')).toBe('true');
      button(label).click();
      await Promise.resolve();
      expect(view.state.doc.toString()).toBe(source.replace('\\]](', `\\]${suffix}](`));
      expect(image().alt).toBe('图片[说明]');
      expect(image().title).toBe('标题');
      expect(image().dataset.imageSize).toBe(size);
      current = label;
    }

    menu(root.querySelector<HTMLImageElement>('img[alt="行内图"]')!);
    expect(button('图片尺寸')).toBeNull();
    menu();
    button('从笔记中移除').click();
    await Promise.resolve();
    expect(view.state.doc.toString()).toBe(source.replace(`![图片\\[说明\\]](${path} "标题")\n`, ''));
    expect(image()).toBeNull();
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(source);
  } finally { view.destroy(); root.remove(); }
});

it('opens the standalone image menu on a touch hold and cancels when the finger moves', () => {
  vi.useFakeTimers();
  const html = document.documentElement;
  const wasMobile = html.hasAttribute('data-mobile');
  html.setAttribute('data-mobile', '');
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const source = '# 图片\n\n![示例](/example.webp)';
  try {
    editor.activate(source, 'touch-image-menu');
    const image = root.querySelector('.cm-markra-image')!;
    const touch = (type: string, clientX = 10) => image.dispatchEvent(Object.assign(
      new MouseEvent(type, { bubbles: true, cancelable: true, clientX, clientY: 20 }),
      { pointerType: 'touch', isPrimary: true, pointerId: 1 },
    ));
    touch('pointerdown');
    touch('pointermove', 30);
    vi.advanceTimersByTime(500);
    expect(document.querySelector('[role="menu"][aria-label="图片操作"]')).toBeNull();
    touch('pointerup');
    touch('pointerdown');
    vi.advanceTimersByTime(500);
    const menu = document.querySelector('[role="menu"][aria-label="图片操作"]')!;
    expect(menu).not.toBeNull();
    touch('pointerup');
    image.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
    expect(root.querySelector('.markra-image-node-source')).toBeNull();
    expect(view.state.doc.toString()).toBe(source);
    menu.querySelector('button')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
  } finally { view.destroy(); root.remove(); html.toggleAttribute('data-mobile', wasMobile); vi.useRealTimers(); }
});

it('passes the right-click selection and the same source to the picture action', async () => {
  const source = '# 笔记\n\n两行之间的选择\n\n以后';
  const root = document.body.appendChild(document.createElement('div'));
  let received: unknown;
  const editor = createRichEditor(root, () => {}, { sharePicture: (text, selection) => { received = [text, selection.from, selection.to]; } });
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(source, 'picture');
    const from = source.indexOf('选择');
    view.dispatch({ selection: { anchor: from, head: from + 2 } });
    root.querySelector('.cm-line')!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    document.querySelector<HTMLButtonElement>('button[aria-label="分享为图片…"]')!.click();
    await Promise.resolve();
    expect(received).toEqual([source, from, from + 2]);
  } finally { view.destroy(); root.remove(); }
});

it('shares a visible text selection when the context menu opens before CodeMirror reads it', async () => {
  const source = '# 笔记\n\n选区验收：中文与 **粗体**。\n\n选区之后的内容。';
  const root = document.body.appendChild(document.createElement('div'));
  let received: unknown;
  const editor = createRichEditor(root, () => {}, { sharePicture: (text, selection) => { received = [text, selection.from, selection.to]; } });
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const selection = document.getSelection()!;
  try {
    editor.activate(source, 'picture-native-selection');
    const line = [...root.querySelectorAll('.cm-line')].find(line => line.textContent?.startsWith('选区验收'))!;
    const text = line.firstChild!;
    const range = document.createRange();
    range.setStart(text, 2);
    range.setEnd(text, 4);
    selection.addRange(range);
    expect(view.state.selection.main.empty).toBe(true);
    line.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    const menu = document.querySelector('[role="menu"][aria-label="编辑操作"]')!;
    expect(menu.querySelector<HTMLButtonElement>('[aria-label="复制"]')?.disabled).toBe(false);
    document.querySelector<HTMLButtonElement>('button[aria-label="分享为图片…"]')!.click();
    await Promise.resolve();
    const from = source.indexOf('验收');
    expect(received).toEqual([source, from, from + 2]);
  } finally { selection.removeAllRanges(); view.destroy(); root.remove(); }
});

it('shares the selected table instead of a previous selection in the note body', async () => {
  const table = '| 项目 | 数值 |\n| --- | --- |\n| 巡检 | 2 |';
  const source = `# 笔记\n\n之前的选区\n\n${table}\n\n之后的内容`;
  const root = document.body.appendChild(document.createElement('div'));
  let received: unknown;
  const editor = createRichEditor(root, () => {}, { sharePicture: (text, selection) => { received = text.slice(selection.from, selection.to); } });
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const selection = document.getSelection()!;
  try {
    editor.activate(source, 'picture-table-selection');
    const from = source.indexOf('之前');
    view.dispatch({ selection: { anchor: from, head: from + 2 } });
    const cell = root.querySelector('td')!;
    const range = document.createRange();
    range.selectNodeContents(cell);
    selection.removeAllRanges();
    selection.addRange(range);
    expect(editor.pictureSelection()?.from).toBe(source.indexOf(table));
    cell.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    document.querySelector<HTMLButtonElement>('button[aria-label="分享为图片…"]')!.click();
    await Promise.resolve();
    expect(received).toBe(table);
    selection.collapse(cell, 0);
    expect(editor.pictureSelection()).toBeUndefined();
  } finally { selection.removeAllRanges(); view.destroy(); root.remove(); }
});

it.each([
  ['添加行', 0, 1, 2, 0],
  ['添加列', 0, 1, -1, 3],
  ['Tab', 1, 2, 2, 0],
  ['Enter', 0, 1, 1, 1],
  ['Enter', 1, 1, 2, 1],
] as const)('moves the grid caret after %s from row %i column %i', async (action, row, column, toRow, toColumn) => {
  const source = '# 表格\n\n| Label | Count | Status |\n| --- | ---: | --- |\n| First | 1 | **Ready** |\n| Second | 2 | Open |\n\n后文';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(source, 'table-caret');
    const cell = root.querySelector<HTMLElement>(`td[data-table-row="${row}"][data-table-column="${column}"]`)!;
    cell.focus();
    const text = document.getSelection()!;
    text.collapse(cell, 0);
    if (action === 'Enter' || action === 'Tab') {
      cell.dispatchEvent(new KeyboardEvent('keydown', { key: action, isComposing: true, bubbles: true, cancelable: true }));
      expect(view.state.doc.toString()).toBe(source);
      expect(cell.contains(text.anchorNode)).toBe(true);
      cell.dispatchEvent(new KeyboardEvent('keydown', { key: action, bubbles: true, cancelable: true }));
    } else root.querySelector<HTMLButtonElement>(`button[aria-label="${action}"]`)!.click();
    await Promise.resolve();
    const expected = root.querySelector(`[data-table-row="${toRow}"][data-table-column="${toColumn}"]`);
    expect(expected).not.toBeNull();
    expect(expected?.contains(text.anchorNode)).toBe(true);
    expect(view.state.doc.toString()).toContain('| First | 1 | **Ready** |');
    expect(view.state.doc.toString().endsWith('\n\n后文')).toBe(true);
    if (action === '添加行') {
      const withRow = view.state.doc.toString();
      root.querySelector<HTMLButtonElement>('button[aria-label="添加列"]')!.click();
      await Promise.resolve();
      root.querySelector('th[data-table-column="3"]')!.dispatchEvent(new InputEvent('beforeinput', { inputType: 'historyUndo', bubbles: true, cancelable: true }));
      expect(view.state.doc.toString()).toBe(withRow);
      expect(document.activeElement?.matches('.cm-markra-table th[data-table-column="2"]')).toBe(true);
    }
    if (toRow === 2 || toColumn === 3) {
      root.querySelector(`[data-table-row="${toRow}"][data-table-column="${toColumn}"]`)!
        .dispatchEvent(new KeyboardEvent('keydown', { key: 'z', metaKey: true, bubbles: true, cancelable: true }));
      expect(view.state.doc.toString()).toBe(source);
    }
  } finally { view.destroy(); root.remove(); }
});

it.each(['行选项', '列选项'])('can remove a one-cell table through %s and undo it', async handle => {
  const source = '# 表格\n\n| 唯一 |\n| --- |\n\n后文';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(source, 'single-cell-table');
    const cell = root.querySelector('th')!;
    cell.focus();
    document.getSelection()!.collapse(cell, 0);
    root.querySelector<HTMLButtonElement>(`button[aria-label="${handle}"]`)!.click();
    expect(document.querySelector<HTMLButtonElement>('button[aria-label="删除行"], button[aria-label="删除列"]')!.disabled).toBe(true);
    document.querySelector<HTMLButtonElement>('button[aria-label="删除表格"]')!.click();
    await Promise.resolve();
    expect(root.querySelector('table')).toBeNull();
    expect(view.state.doc.toString()).toContain('后文');
    const notice = root.querySelector('.editor-history-notice')!;
    expect(notice.querySelector('[role="status"]')?.textContent).toBe('已删除表格');
    const button = notice.querySelector('button')!;
    if (handle === '行选项') {
      view.dispatch({ changes: { from: view.state.doc.length, insert: '。' }, userEvent: 'input.type' });
      expect(notice.isConnected).toBe(false);
      button.click();
      expect(view.state.doc.toString()).toContain('后文。');
      expect(undo(view)).toBe(true);
      expect(undo(view)).toBe(true);
    } else button.click();
    expect(view.state.doc.toString()).toBe(source);
    expect(notice.isConnected).toBe(false);
  } finally { view.destroy(); root.remove(); }
});

it('promotes the first data row when a header is deleted and preserves column alignment', async () => {
  const source = '# 表格\n\n| Label | Count |\n| --- | ---: |\n| First | 1 |\n| Second | 2 |\n\n后文';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(source, 'delete-table-header');
    const header = root.querySelectorAll('th')[1];
    header.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    document.querySelector<HTMLButtonElement>('button[aria-label="删除行"]')!.click();
    await Promise.resolve();
    await Promise.resolve();
    expect(view.state.doc.toString()).toBe('# 表格\n\n| First | 1 |\n| --- | ---: |\n| Second | 2 |\n\n后文');
    expect(root.querySelectorAll('th')[1].contains(document.getSelection()!.anchorNode)).toBe(true);
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(source);
  } finally { view.destroy(); root.remove(); }
});

it('shows the language once while idle and reveals both editable fences only in the active code block', () => {
  const source = '# 代码示例\n\n```js\nconst greeting = "你好";\n```\n\n~~~sql\nselect 1;\n~~~\n\n后文';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const labels = () => [...root.querySelectorAll('.cm-markra-code-language')].map(label => label.textContent);
  try {
    editor.activate(source, 'code-fence-look');
    view.focus();
    expect(labels()).toEqual(['js', 'sql']);
    expect(root.textContent).not.toContain('```');
    expect(root.querySelector('.markra-code-language-select')).toBeNull();
    expect(root.querySelector('.cm-markra-code-token')).not.toBeNull();
    view.dispatch({ selection: { anchor: source.indexOf('greeting') } });
    expect(labels()).toEqual(['sql']);
    expect(root.textContent?.match(/```/g)).toHaveLength(2);
    expect(root.textContent).not.toContain('~~~');
    const from = source.indexOf('```js') + 3;
    view.dispatch({ changes: { from, to: from + 2, insert: 'ts' }, selection: { anchor: from + 2 } });
    view.dispatch({ selection: { anchor: view.state.doc.length } });
    expect(labels()).toEqual(['ts', 'sql']);
    expect(view.state.doc.toString()).toBe(source.replace('```js', '```ts'));
  } finally { view.destroy(); root.remove(); }
});

it('turns typed checkbox shorthand into a task only in editable prose', () => {
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    for (const [before, prefix, after] of [
      ['', '[]', '- [ ] '], ['', '[ ]', '- [ ] '], ['', '- []', '- [ ] '], ['', '[X]', '- [x] '],
      ['- 外层\n', '  + []', '  - [ ] '], ['', '+ [ ]', '+ [ ] '],
      ['```md\n', '[]', '[] '], ['', '    []', '    [] '], ['', '字面 []', '字面 [] '],
    ]) {
      const tail = before.startsWith('```') ? '\n```' : '';
      const source = before + prefix + tail;
      editor.activate(source, source);
      const at = before.length + prefix.length;
      view.dispatch({ selection: { anchor: at } });
      const insert = () => view.state.update({ changes: { from: at, insert: ' ' }, selection: { anchor: at + 1 }, userEvent: 'input.type' });
      if (!view.state.facet(EditorView.inputHandler).some(handler => handler(view, at, at, ' ', insert))) view.dispatch(insert());
      expect(view.state.doc.toString()).toBe(before + after + tail);
      expect(view.state.selection.main.head).toBe(before.length + after.length);
      if (after.includes('- [')) {
        const task = root.querySelector<HTMLInputElement>('.cm-markra-task-checkbox');
        expect(task?.checked).toBe(after.includes('[x]'));
      }
      expect(undo(view)).toBe(true);
      expect(view.state.doc.toString()).toBe(source);
    }
  } finally { view.destroy(); root.remove(); }
});

it('leaves a code block on the last empty line and removes that line', () => {
  const source = '# 代码\n\n```js\nconst value = 2;\n\n```\n\n后文';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(source, 'leave-code');
    view.focus();
    view.dispatch({ selection: { anchor: source.indexOf('\n\n```', source.indexOf('const')) + 1 } });
    view.contentDOM.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    const expected = source.replace('2;\n\n```', '2;\n```');
    expect(view.state.doc.toString()).toBe(expected);
    expect(view.state.selection.main.head).toBe(expected.indexOf('```\n\n') + 4);
  } finally { view.destroy(); root.remove(); }
});

it('copies only the code from every line of the clicked fenced block', async () => {
  const source = '# 代码\n\n> ```js\n> const greeting = "你好";\n> \n> console.log(greeting);\n> ```\n\n后文';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const writeText = vi.fn().mockResolvedValue(undefined);
  const clipboard = Object.getOwnPropertyDescriptor(navigator, 'clipboard');
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
  try {
    editor.activate(source, 'copy-code');
    root.querySelector('.cm-markra-code-content-line')!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    document.querySelector<HTMLButtonElement>('button[aria-label="复制代码"]')!.click();
    await Promise.resolve();
    expect(writeText).toHaveBeenCalledWith('const greeting = "你好";\n\nconsole.log(greeting);');
    expect(view.state.doc.toString()).toBe(source);
  } finally {
    if (clipboard) Object.defineProperty(navigator, 'clipboard', clipboard);
    else Reflect.deleteProperty(navigator, 'clipboard');
    view.destroy(); root.remove();
  }
});

it.each([
  ['复制代码', '.cm-markra-code-content-line', 'const greeting = "你好";'],
  ['复制链接', '.qingyu-link-card', 'https://example.com/docs'],
] as const)('copies from the macOS native menu via %s', async (label, selector, expected) => {
  const source = '# 笔记\n\n<https://example.com/docs>\n\n```js\nconst greeting = "你好";\n```';
  const root = document.body.appendChild(document.createElement('div'));
  const html = document.documentElement;
  const platform = html.dataset.platform, wasMobile = html.hasAttribute('data-mobile');
  html.dataset.platform = 'macos'; html.removeAttribute('data-mobile');
  const invoke = vi.fn().mockResolvedValue(null);
  vi.stubGlobal('__TAURI_INTERNALS__', { invoke });
  const popup = vi.fn().mockResolvedValue(undefined);
  const close = vi.fn().mockResolvedValue(undefined);
  const menu = vi.spyOn(Menu, 'new').mockResolvedValue({ popup, close } as unknown as Menu);
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(source, 'native-context-copy');
    root.querySelector(selector)!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(popup).toHaveBeenCalledOnce());
    const item = menu.mock.calls[0][0]?.items?.find(entry => typeof entry === 'object' && 'text' in entry && entry.text === label);
    if (!item || !('action' in item) || !item.action) throw new Error('Missing copy action in native menu');
    item.action('copy');
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith('table_clipboard', { text: expected }, undefined));
    expect(view.state.doc.toString()).toBe(source);
  } finally {
    menu.mockRestore(); vi.unstubAllGlobals();
    if (platform === undefined) delete html.dataset.platform;
    else html.dataset.platform = platform;
    html.toggleAttribute('data-mobile', wasMobile);
    view.destroy(); root.remove();
  }
});

it('copies and cuts a whole cell without a text selection, and pastes into an empty cell', async () => {
  const source = '| Name | Count |\n| --- | ---: |\n| 中文 \\| 保留 | 1 |\n| 右边不变 | 2 |';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const writeText = vi.fn().mockResolvedValue(undefined);
  const readText = vi.fn().mockResolvedValue('第一行\r\n\r\n第二行 \\| A');
  const clipboard = Object.getOwnPropertyDescriptor(navigator, 'clipboard');
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText, readText } });
  const open = () => {
    const cell = root.querySelector('td')!;
    placeTableCellCaret(cell, 0);
    cell.dispatchEvent(new MouseEvent('mousedown', { button: 2, bubbles: true }));
    const word = cell.firstChild;
    if (word?.nodeType === Node.TEXT_NODE && word.textContent!.length >= 2) {
      const selection = document.getSelection()!;
      selection.setBaseAndExtent(word, 0, word, 2);
    }
    cell.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
  };
  const item = (label: string) => document.querySelector<HTMLButtonElement>(`[role="menu"][aria-label="表格操作"] [aria-label="${label}"]`)!;
  try {
    editor.activate(source, 'cell-clipboard');
    open();
    item('复制').click();
    await Promise.resolve();
    expect(writeText).toHaveBeenCalledWith('中文 | 保留');
    expect(view.state.doc.toString()).toBe(source);

    open();
    item('剪切').click();
    const empty = source.replace('中文 \\| 保留', '');
    await vi.waitFor(() => expect(view.state.doc.toString()).toBe(empty));
    open();
    expect(item('复制').disabled).toBe(true);
    expect(item('剪切').disabled).toBe(true);
    item('粘贴').click();
    await vi.waitFor(() => expect(view.state.doc.toString()).toBe(source.replace('中文 \\| 保留', `第一行 第二行 ${'\\'.repeat(3)}| A`)));
    expect(root.querySelector('td')!.textContent).toBe('第一行 第二行 \\| A');
    open();
    item('复制').click();
    await Promise.resolve();
    expect(writeText).toHaveBeenLastCalledWith('第一行 第二行 \\| A');
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(empty);
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe(source);
  } finally {
    if (clipboard) Object.defineProperty(navigator, 'clipboard', clipboard);
    else Reflect.deleteProperty(navigator, 'clipboard');
    view.destroy(); root.remove();
  }
});

it.each(['ArrowDown', 'ArrowUp'] as const)('uses %s to enter the grid, move within it and leave it', key => {
  const source = '# 笔记\n\n正文上方\n\n| 左表头 | 右表头 |\n| --- | --- |\n| 左一 | 右一 |\n| 左二 | 右二 |\n\n正文下方';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const press = (target: Element, isComposing = false) => target.dispatchEvent(new KeyboardEvent('keydown', { key, isComposing, bubbles: true, cancelable: true }));
  const down = key === 'ArrowDown';
  const cell = (row: number, column: number) => root.querySelector<HTMLTableCellElement>(`[data-table-row="${row}"][data-table-column="${column}"]`)!;
  try {
    editor.activate(source, 'table-vertical-navigation');
    view.focus();
    view.dispatch({ selection: { anchor: view.state.doc.line(down ? 4 : 9).from } });
    vi.spyOn(view, 'moveVertically').mockImplementation(start => start);
    press(view.contentDOM);
    expect(document.activeElement?.textContent).toBe(down ? '左表头' : '左二');

    const secondColumn = cell(down ? -1 : 1, 1);
    secondColumn.focus();
    document.getSelection()!.collapse(secondColumn, 0);
    press(secondColumn, true);
    expect(document.activeElement).toBe(secondColumn);
    press(secondColumn);
    expect(document.activeElement).toBe(cell(0, 1));
    press(cell(0, 1));
    expect(document.activeElement).toBe(cell(down ? 1 : -1, 1));
    press(cell(down ? 1 : -1, 1));
    expect(document.activeElement).toBe(view.contentDOM);
    expect(view.state.doc.lineAt(view.state.selection.main.head).number).toBe(down ? 10 : 4);
    expect(view.state.doc.toString()).toBe(source);
  } finally { view.destroy(); root.remove(); }
});

it('exits the last table with Escape without undoing cell edits', async () => {
  const source = '# 笔记\n\n| 表头 |\n| --- |\n| 修改前 |';
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  try {
    editor.activate(source, 'leave-table');
    let cell = root.querySelector('td')!;
    cell.focus();
    cell.textContent = '已提交';
    document.getSelection()!.collapse(cell.firstChild, 3);
    cell.dispatchEvent(new InputEvent('input', { bubbles: true, inputType: 'insertText' }));
    await Promise.resolve();
    cell = root.querySelector('td')!;
    cell.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    expect(view.state.doc.toString()).toBe(source.replace('修改前', '已提交') + '\n\n');
    expect(view.state.selection.main.head).toBe(view.state.doc.length);
    expect(document.activeElement).toBe(view.contentDOM);
    view.dispatch(view.state.replaceSelection('表格之后'));
    expect(root.querySelectorAll('.cm-markra-table td')).toHaveLength(1);
    expect([...root.querySelectorAll('.cm-markra-paragraph')].some(line => line.textContent === '表格之后')).toBe(true);
  } finally { view.destroy(); root.remove(); }
});

it.each([
  ['行选项', '| Label | Count |\n| --- | ---: |\n|  |  |\n| Second | 2 |', 2],
  ['列选项', '| Label |  |\n| --- | ---: |\n| First |  |\n| Second |  |', 3],
] as const)('keeps %s picked after dismissing its menu and clears only that line with Delete', (handle, expected, pickedCount) => {
  const table = '| Label | Count |\n| --- | ---: |\n| First | 1 |\n| Second | 2 |';
  const source = `# 笔记\n\n${table}\n\n后文`;
  const root = document.body.appendChild(document.createElement('div'));
  const editor = createRichEditor(root, () => {});
  const view = EditorView.findFromDOM(root.querySelector('.cm-editor')!)!;
  const press = (key: string, target: Element, metaKey = false) => target.dispatchEvent(new KeyboardEvent('keydown', { key, metaKey, bubbles: true, cancelable: true }));
  const cell = () => root.querySelector<HTMLTableCellElement>('td[data-table-row="0"][data-table-column="1"]')!;
  const button = () => root.querySelector<HTMLButtonElement>(`button[aria-label="${handle}"]`)!;
  const selectionCount = () => root.querySelectorAll('[data-table-line-picked]').length;
  try {
    editor.activate(source, 'picked-table-line');
    cell().focus();
    document.getSelection()!.collapse(cell(), 1);
    button().click();
    expect(selectionCount()).toBe(pickedCount);
    expect(button().ariaPressed).toBe('true');
    press('Escape', document.activeElement!);
    expect(document.activeElement).toBe(cell());
    if (handle === '行选项') press('Backspace', cell());
    else cell().dispatchEvent(new InputEvent('beforeinput', { inputType: 'deleteContentForward', bubbles: true, cancelable: true }));
    expect(view.state.doc.toString()).toBe(source.replace(table, expected));
    expect(selectionCount()).toBe(pickedCount);
    press('z', cell(), true);
    expect(view.state.doc.toString()).toBe(source);
    expect(selectionCount()).toBe(pickedCount);
    cell().dispatchEvent(new MouseEvent('mousedown', { bubbles: true }));
    expect(selectionCount()).toBe(0);
    expect(button().ariaPressed).toBe('false');
  } finally { view.destroy(); root.remove(); }
});
